# P3.7-J3 Counterfactual Shadow Probe Plane

Date: 2026-05-19

Status: Accepted plan with P0R and J3R corrective repairs

## Decision

Prepare P3.7-J3 as a separate counterfactual shadow-only probe plane.

Full R14 remains HOLD until P3.7 has rows with:

- V3/MFS payload,
- shadow transport / entry / lifecycle evidence,
- stable `ab_record_id` / probe join keys,
- no active policy mutation.

The probe plane is a collection mechanism. It is not a selector prototype and
not a BUY path.

## Context

P3.7-J2 confirmed that the R14 profile can emit V3/MFS replay rows, and strict
full replay passed. It did not observe natural Gatekeeper BUYs:

- V3/V2.5 rows: `505`
- strict full replay: `PASS`
- decision-side `ab_record_id` and V3 hash coverage: `100%`
- shadow transport rows: `0`
- shadow entry rows: `0`
- shadow lifecycle rows: `0`

P3.7-J2b validated the shadow dispatch join-key path at code/test harness
level, but not with runtime shadow rows.

The active BUY path is still Gatekeeper V2/V2.5 long-mode plus IWIM. V3 remains
a telemetry/replay sidecar with promotion disabled.

Waiting for natural BUYs is impractical for P3.7 dataset collection. Threshold
tuning and IWIM changes are forbidden. Therefore the next safe option is a
separate counterfactual shadow-only probe plane.

## P0R Corrective Repair

P0R is a corrective sub-stage after the P3.7-J3 P0 audit. The audit found that
the original P0 implementation proved only a narrow harness path:

- synthetic `counterfactual_probe_recorded_no_simulation` rows were treated too
  strongly in reporting,
- runtime probe dispatch was not yet routed through the real shadow simulator,
- probe runtime bounds were config-only rather than enforced,
- append/namespace semantics were not fully fail-closed,
- the join-key audit could pass selection/transport/entry continuity without an
  exact join back to the decision/V3 row.

P0R changes the status model:

```text
Original J3 P0: PARTIAL harness PASS / superseded by P0R
J3 P0R code-level repair: target PASS
R15 runtime smoke: next gate, not claimed complete by P0R
Full R14 / Phase B / P2 / live: HOLD / NO-GO
```

P0R implementation scope:

- add `run_id` and `session_id` to `[p37_shadow_probe]`;
- fail closed on existing probe outputs when `append=false`;
- require `run_id` and `session_id` when `append=true`;
- enforce `max_probes_per_run`, `max_probes_per_minute`, `max_concurrent`, and
  `dedupe_by_probe_id` in shared runtime state;
- log bounded-runtime skips as probe skip rows, not active decision failures;
- route selected probes through a probe-only `TriggerComponent` helper that
  calls `shadow_simulator.simulate_buy(...)`;
- enforce `probe_amount_source="fixed_lamports"` by passing
  `probe_amount_lamports` into the prepared shadow request, rather than only
  logging the configured amount;
- avoid `dispatch_prepared_buy_shadow_only`, active position reservation, live
  sender paths, and active BUY logs;
- keep the synthetic no-simulation row builder only as a harness fixture, not as
  P0 success evidence;
- fail closed at startup when `[p37_shadow_probe].enabled=true` but the
  configured Ghost Brain file does not have
  `[gatekeeper_v3].replay_payload_enabled=true`;
- add probe dispatch/amount/source/bucket/run/session fields to transport and
  entry rows;
- require probe artifacts to exact-join to decision/V3 rows by `ab_record_id`
  and V3 hashes before the audit can return PASS;
- reduce the R15 smoke profile to bounded probe limits.

P0R does not run or claim the R15 runtime smoke. R15 remains the next gate.

## J3R Counterfactual Probe Runtime Repair

J3R is a narrow corrective stage opened after the first R15 counterfactual
probe smoke. The smoke was useful but did not reach minimal PASS:

- V3/MFS replay path passed with strict replay OK.
- Probe selection and probe transport rows were emitted.
- Active BUY rows remained unchanged.
- No probe entry/lifecycle rows were emitted because all probe simulations
  ended in `AccountNotFound` / `data_problem`.
- Probe transport exact decision/V3 continuity was only partial.
- `probe_skips.jsonl` showed a concurrent append robustness issue.

J3R changes the status model:

```text
J3 P0R code-level repair: PASS with runtime smoke findings
J3R code-level repair: target PASS
R15-r2 runtime smoke: next gate, not claimed complete by J3R
Full collection / Phase B / P2 / live: HOLD / NO-GO
```

J3R implementation scope:

- compute the probe candidate feature hash using the same serialized V3 replay
  payload boundary used by persisted decision rows, instead of trusting a
  pre-serialization hash field;
- propagate source decision metadata through selection, transport, and entry:
  source decision plane, source V3 feature hash, source V3 policy hash, and
  optional source log path/row metadata when available;
- prefer source metadata from the probe selection record when writing transport
  rows, so transport does not silently recompute or substitute a different V3
  hash;
- extend transport error rows with simulation diagnostics: error kind/message,
  missing account pubkey/role when available, account override presence,
  bonding curve, payer, token program, ATA, curve/mint/payer availability,
  curve/account diagnostics, and explicit precheck failure reason;
- add a lightweight probe execution precheck that converts known incomplete
  execution state into `probe_skipped` with
  `skip_reason=probe_execution_precheck_failed`, instead of attempting a
  shadow simulation that can only fail as an opaque data problem;
- serialize probe selection/skip/transport/entry JSONL writes through a shared
  writer lock so concurrent probe tasks cannot concatenate JSON objects into a
  single physical line;
- harden the join-key audit so a PASS requires exact join to persisted
  decision/V3 rows by `ab_record_id`, source decision plane, policy hash, and
  feature hash, with explicit mismatch reason counts;
- keep legacy/degraded parsing for old probe rows without `ab_record_id`,
  `probe_id`, or source metadata;
- introduce a fresh bounded R15-r2 smoke namespace:
  `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r2`.

J3R does not run or claim the R15-r2 runtime smoke. A successful J3R code-level
repair only authorizes a bounded R15-r2 smoke. It does not authorize broad
collection, Phase B, P2, live execution, active policy changes, IWIM changes, or
threshold tuning.

## J3R2 Counterfactual Probe Simulation and Hash Continuity Repair

J3R2 is a second narrow corrective stage opened after the R15-r2 bounded smoke.
The R15-r2 smoke confirmed that V3/MFS replay and probe selection/transport
logging work, but it did not reach minimal runtime PASS:

- `v3_rows=79`, strict replay OK, `bad_rows=0`;
- `probe_selection_rows=5`;
- `probe_transport_rows=5`;
- `probe_entry_rows=0`;
- all probe simulations ended with `AccountNotFound` / `data_problem`;
- exact decision/V3 hash continuity was only `1/5`.

J3R2 changes the status model:

```text
R15-r2 runtime smoke: NOT_READY
J3R2 code-level repair: target PASS
R15-r3 runtime smoke: next gate, not claimed complete by J3R2
Full collection / Phase B / P2 / live: HOLD / NO-GO
```

J3R2 implementation scope:

- compute probe candidate `v3_feature_snapshot_hash` from the same
  post-serialize JSON boundary used by persisted decision rows;
- keep active DecisionLogger hashing unchanged;
- add a probe-only required-account precheck after `PreparedBuyRequest` is built
  and before `shadow_simulator.simulate_buy(...)`;
- inspect the prepared transaction account set plus explicit request identities
  and classify a known missing account as a `probe_skipped` row with
  `skip_reason=probe_execution_precheck_failed` and
  `precheck_failure_reason=missing_required_account:<role>:<pubkey>`;
- do not treat `AccountNotFound` as success and do not write probe entry rows
  for failed simulations;
- preserve idempotent ATA creation semantics by not classifying a missing user
  ATA as fatal when the prepared transaction creates it;
- leave probe precheck RPC errors fail-open for active decisions: log the
  precheck failure and continue to simulation instead of mutating the active
  verdict path;
- add targeted tests for post-serialize hash use and required-account role
  classification.

J3R2 does not run or claim R15-r3. A successful J3R2 code-level repair only
authorizes a fresh bounded smoke namespace. It does not authorize broad
collection, Phase B, P2, live execution, active policy changes, IWIM changes, or
threshold tuning.

## Goal

Collect a forward-only research dataset where sampled V3/MFS decision rows get
shadow simulation/lifecycle probes without changing active verdicts.

Target dataset shape:

```text
V3/MFS decision snapshot
+ V3 replay payload hashes
+ active V2/V2.5 verdict context
+ counterfactual shadow probe transport
+ counterfactual shadow entry/lifecycle
+ shadow-onchain lifecycle truth
+ lifecycle labels
+ stable join keys
```

## Non-Goals

J3 does not authorize:

- P2,
- live execution,
- active Gatekeeper changes,
- IWIM changes,
- live sender changes,
- threshold tuning,
- V3 promotion,
- MFS extension as policy,
- treating probe dispatch as BUY,
- treating lifecycle outcomes as decision-time features,
- treating shadow simulation as live inclusion,
- treating speculative finality as finalized proof.

## Required Semantics

The implementation must preserve these semantics:

- Active verdict remains unchanged.
- `no dispatch after reject` remains normal active behavior.
- Probe dispatch is separate and must carry
  `dispatch_source=counterfactual_shadow_probe`.
- Probe rows are research artifacts, not active BUY rows.
- Source `ab_record_id` joins the probe back to the V3/MFS decision row.
- `probe_id` identifies a specific counterfactual probe attempt.
- Lifecycle outcome is a post-decision label.
- Unknown execution status is not success.
- Speculative finality remains dirty/degraded.

## Config Design

Add a disabled-by-default launcher config section:

```toml
[p37_shadow_probe]
enabled = false
namespace = "shadow-burnin-v3-p37-counterfactual-probe-r15-smoke"
dispatch_source = "counterfactual_shadow_probe"
sample_source = "v3_mfs_decision_rows"
sample_mode = "deterministic_hash_mod"
sample_modulus = 100
sample_threshold = 5
sampling_version = "p37-j3-v1"
run_id = ""
session_id = ""
max_probes_per_run = 25
max_probes_per_minute = 10
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

selection_log_path = "../../logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke/probe_selection.jsonl"
skip_log_path = "../../logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke/probe_skips.jsonl"
transport_log_path = "../../logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke/probe_transport.jsonl"
entry_log_path = "../../logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke/probe_shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke/probe_shadow_lifecycle.jsonl"
```

Backward compatibility:

- All new fields must use serde defaults.
- Old configs must load with `p37_shadow_probe.enabled=false`.
- Legacy shadow artifacts without probe fields must still parse.

Fail-closed validation when enabled:

- `[execution].execution_mode` must be `shadow`.
- `[trigger].entry_mode` must be `shadow_only`.
- `[trigger.shadow_run].enabled` must be `true`.
- V3 replay payload emission must be enabled.
- Probe paths must not equal active `buys.jsonl`, `shadow_entries.jsonl`, or
  `shadow_lifecycle.jsonl` paths.
- Live/P2 promotion must remain disabled.
- If `append=false`, existing output files fail closed.
- If `append=true`, `run_id` and `session_id` are required in every row.

## Required Safety Amendments Before P0

The following amendments are mandatory before runtime implementation:

1. Probe eligibility precheck before sampling.
2. `probe_bucket` / stratified sampling metadata.
3. EventBus isolation: disabled in P0 or dedicated probe event types only.
4. Probe ledger / position state isolation.
5. Explicit probe amount, quote age, and slippage config.
6. Fail-open backpressure for active decisions.
7. Unique namespace and path collision protection.
8. Strict active BUY non-mutation tests.
9. P0/P1 split: selection/transport/entry first, lifecycle labels after close.
10. Collision-safe `probe_id`.

## Probe Eligibility Precheck

Eligibility precheck is a technical guard, not active policy.

Minimum requirements:

- valid `pool_id`,
- valid `base_mint` / `mint_id`,
- valid bonding curve or execution route identity,
- `MaterializedFeatureSet` present,
- `v3_feature_snapshot_hash` present,
- `v3_policy_config_hash` present,
- transaction/account identity available,
- curve/account state not critically unavailable,
- protocol state supported by the shadow simulator,
- no duplicate `probe_id`.

Eligibility skip reasons:

- `invalid_pool_identity`
- `invalid_mint_identity`
- `unsupported_protocol_state`
- `missing_bonding_curve`
- `missing_execution_route_identity`
- `missing_materialized_feature_set`
- `critical_curve_unavailable`
- `critical_account_unavailable`
- `duplicate_probe_id`

## Probe Buckets And Stratified Sampling

Every selected or skipped row must carry:

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

P0 may still sample only `REJECT` and `PENDING` verdict families, but reports
must segment by bucket. Bucketless probe labels are not sufficient for P3.7
feature interpretation.

## EventBus Isolation

P0 default:

```text
emit_event_bus = false
event_bus_mode = "disabled"
```

If EventBus emission is implemented later, it must use distinct probe-only event
types:

```text
CounterfactualShadowProbeRequested
CounterfactualShadowProbeCompleted
CounterfactualShadowProbeSkipped
```

Forbidden event reuse:

- active BUY event,
- trigger BUY event,
- position-opened event,
- live BUY event,
- any event consumed by active execution subscribers.

## Probe Ledger / Position State Isolation

Probe positions must be isolated from active shadow/live position state.

Requirements:

- `probe_position_id` must be distinct from active `position_id`.
- probe lifecycle must be written to the probe lifecycle path.
- probe dispatch must not increment active open position count.
- probe dispatch must not affect `max_concurrent_positions`.
- probe dispatch must not mutate active shadow/live ledger state unless the
  state is explicitly namespaced as a probe ledger.

If the existing post-buy monitor cannot isolate probe position state safely,
P0 stops at selection, transport, and entry. Lifecycle monitoring moves to P1.

## Probe Amount / Quote / Slippage Contract

Probe lifecycle PnL and price impact depend on amount and quote parameters.
They must be explicit.

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

## Fail-Open Backpressure

Probe queue, rate, or concurrency pressure must skip the probe and must not
block the active decision pipeline.

Required skip reasons:

- `probe_queue_full`
- `probe_backpressure`
- `probe_rate_limit_exceeded`
- `probe_concurrency_limit_exceeded`

Acceptance:

- active decision latency is not blocked by the probe queue,
- probe enqueue failure does not alter active verdict,
- probe backpressure writes `probe_skipped`, not active failure.

## Namespace And Output Path Protection

Required behavior:

- probe namespace must be unique per run,
- probe paths must not collide with active decision, active BUY, shadow entry,
  shadow lifecycle, or historical report paths,
- if `append=false` and output files already exist, fail closed,
- if `append=true`, every row must include `run_id` and `session_id`.

## Minimal P0 Runtime Plan

### P0.1 Config Surface

Files:

- `ghost-launcher/src/config.rs`
- config load tests under existing launcher config tests

Work:

- Add `P37ShadowProbeConfig` with `#[serde(default)]`.
- Add path resolution for probe paths.
- Add validation that fails closed when enabled outside shadow-only execution.
- Add config summary logging that prints `p37_shadow_probe.enabled` and
  namespace.

Acceptance:

- Existing rollout configs still load.
- New probe config loads.
- Enabling probe in non-shadow profile fails closed.

### P0.2 Probe Candidate Selection

Files:

- `ghost-launcher/src/oracle_runtime.rs`
- existing decision logging / V3 replay payload boundary

Work:

- Hook after the row has a `MaterializedFeatureSet`, V3 replay payload, and
  `ab_record_id`.
- Run probe eligibility precheck before sampling.
- Assign `probe_bucket`, `probe_bucket_reason`, and `probe_bucket_version`.
- Evaluate deterministic sampler.
- Write exactly one selection/skip record per eligible decision row.
- Never mutate active verdict or active reason chain.
- Never enqueue active BUY.

Preferred deterministic sample key:

```text
hash(ab_record_id + v3_policy_config_hash + namespace + sampling_version)
```

Collision-safe probe id:

```text
probe_id = hash(source_ab_record_id + sampling_version + probe_bucket + probe_amount_lamports)
```

Acceptance:

- Same input row and config produces the same selection decision.
- Missing metadata produces explicit `probe_skipped`.
- Ineligible technical rows produce explicit precheck skip reasons.
- Bucket assignment is logged for selected and skipped rows.
- Active verdict fields remain byte-for-byte unchanged in decision logs.

### P0.3 Probe Join Metadata

Files:

- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-launcher/src/components/trigger/shadow_run.rs`

Work:

- Reuse or extend `ExecutionJoinMetadata` additively.
- Preserve source `ab_record_id`.
- Add probe-specific fields:
  - `dispatch_source`
  - `collection_plane`
  - `probe_plane`
  - `probe_id`
  - `probe_bucket`
  - `probe_bucket_reason`
  - `probe_bucket_version`
  - `probe_sampling_version`
  - `probe_sample_reason`
  - `source_decision_plane`
  - `active_verdict_type`
  - `active_reason_code`

Acceptance:

- Probe transport JSON includes source `ab_record_id`.
- Probe transport JSON includes `probe_id`.
- Probe rows carry `dispatch_source=counterfactual_shadow_probe`.
- Legacy transport rows without probe fields still parse.

### P0.4 Shadow-Only Probe Dispatch

Files:

- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-launcher/src/components/trigger/shadow_run.rs`

Work:

- Build a shadow-only simulation request from a selected decision row.
- Use only the shadow transport path.
- Write probe transport to `p37_shadow_probe.transport_log_path`.
- Write probe entries to `p37_shadow_probe.entry_log_path`.
- In P0, do not require lifecycle close.
- Hand post-buy monitoring the same join metadata only if probe ledger/state
  isolation is already implemented.
- Bound concurrency and rate.
- Treat queue/rate/concurrency pressure as `probe_skipped`, not active failure.

Important boundary:

The probe dispatcher must not call live dispatch and must not write active
Gatekeeper BUY rows.

Acceptance:

- Synthetic probe dispatch produces transport and entry rows.
- Probe dispatch writes `probe_amount_lamports`, `probe_amount_source`,
  `probe_slippage_bps`, `quote_age_ms`, and `curve_age_ms`.
- No active BUY counter increments because of probe rows.
- No active open position counter increments because of probe rows.
- No live transaction send path is reachable from probe dispatch.

### P1 Lifecycle / On-Chain / Labels Propagation

Files:

- `ghost-brain/src/guardian/post_buy/engine.rs`
- `scripts/shadow_onchain_lifecycle_report.py`
- `scripts/v3_p37_shadow_lifecycle_labeler.py`
- `scripts/v3_p37_shadow_lifecycle_feature_availability.py`

Work:

- Implement only after P0 transport/entry join-key smoke passes.
- Preserve probe join metadata in lifecycle records.
- Propagate probe fields through shadow-onchain lifecycle reports.
- Mark labels with:
  - `collection_plane=counterfactual_shadow_probe`
  - `dispatch_source=counterfactual_shadow_probe`
  - `label_source=counterfactual_shadow_probe_lifecycle`
- Keep speculative finality as dirty/degraded.

Acceptance:

- Lifecycle rows inherit `ab_record_id` and `probe_id`.
- Labeler does not classify probe rows as active BUY rows.
- Feature availability can join labels to V3/MFS rows by exact AB/probe keys.

### P0.5 Join-Key Audit

Files:

- `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`
- `scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py`
- optional new `scripts/test_v3_p37_counterfactual_shadow_probe_audit.py`

Work:

- Add probe artifact inputs or auto-discovery of probe paths from config.
- Report:
  - `probe_selected_rows`
  - `probe_skipped_rows`
  - `probe_transport_rows`
  - `probe_entry_rows`
  - `probe_lifecycle_rows`
  - `probe_rows_with_ab_record_id`
  - `probe_rows_with_probe_id`
  - `probe_bucket` counts
  - `probe_skip_reason` counts
  - `probe_amount_source` counts
  - `exact_ab_record_id` coverage
  - `exact_probe_id` continuity
  - fallback join counts
  - unmatched rows
- Keep legacy rows degraded, not parser failures.

Acceptance:

- Fixture with source decision row and probe artifacts returns `PASS`.
- Fixture without `ab_record_id` parses and returns degraded/not-ready.
- Probe fixture with active BUY-like artifacts but missing dispatch source fails.

## Required Log Fields

### Probe Selection / Skip

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
probe_skip_reason
probe_amount_lamports
probe_amount_source
probe_slippage_bps
quote_age_ms
curve_age_ms
ab_record_id
source_ab_record_id
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
active_verdict_type
active_verdict_buy
active_reason_code
active_reason_chain
v3_shadow_verdict
v3_shadow_reason_code
v3_shadow_confidence
rollout_namespace
```

### Probe Transport

```text
schema_version
collection_plane
dispatch_source
probe_plane
probe_id
probe_bucket
ab_record_id
source_ab_record_id
candidate_id
pool_id
base_mint
mint_id
decision_ts_ms
probe_dispatch_ts_ms
probe_amount_lamports
probe_amount_source
probe_slippage_bps
quote_age_ms
curve_age_ms
v3_feature_snapshot_hash
v3_policy_config_hash
source_decision_plane
decision_plane
rollout_namespace
simulation_status
execution_outcome
```

### Probe Entry

```text
schema_version
collection_plane
dispatch_source
probe_plane
probe_id
probe_bucket
ab_record_id
candidate_id
probe_position_id
pool_id
base_mint
mint_id
decision_ts_ms
entry_execution_ts_ms
entry_price
entry_slot
probe_amount_lamports
probe_amount_source
probe_slippage_bps
quote_age_ms
curve_age_ms
v3_feature_snapshot_hash
v3_policy_config_hash
source_decision_plane
rollout_namespace
```

### Probe Lifecycle

```text
schema_version
collection_plane
dispatch_source
probe_plane
probe_id
probe_bucket
ab_record_id
candidate_id
probe_position_id
pool_id
base_mint
mint_id
decision_ts_ms
entry_execution_ts_ms
close_ts_ms
close_reason
v3_feature_snapshot_hash
v3_policy_config_hash
source_decision_plane
rollout_namespace
```

## Join-Key Contract

Primary feature-to-probe key:

```text
source_ab_record_id == ab_record_id
```

Primary probe continuity key:

```text
probe_id
```

`probe_id` must include source AB, sampling version, bucket, and amount either
directly or through a deterministic hash. This avoids collisions if future probe
variants use different buckets or amounts for the same source row.

Required continuity:

```text
decision row
  -> probe selection
  -> probe transport
  -> probe entry
  -> P0 join-key audit
  -> P1 probe lifecycle
  -> P1 shadow-onchain lifecycle report
  -> P1 lifecycle labels
  -> P1 feature availability audit
```

Fallback keys such as `pool_id + mint + time window` may be reported but must
not be the primary success condition for J3.

## Smoke Plan

After P0 implementation, create a small isolated smoke profile:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke.toml
```

Smoke goals:

- produce V3/MFS rows,
- select a bounded number of counterfactual probes,
- emit probe transport and entry rows,
- preserve AB/probe join metadata,
- avoid active BUY mutation,
- avoid live/P2.
- not require closed lifecycle positions for P0 PASS.

Suggested smoke limits:

```text
max_probes_per_run = 5
max_probes_per_minute = 5
max_concurrent = 1
```

Smoke PASS:

- `v3_rows > 0`
- `full_snapshot_payload_rows == v3_rows`
- `hash_only_rows = 0`
- `probe_selected_rows > 0`
- `probe_transport_rows > 0`
- `probe_entry_rows > 0`
- `probe_transport_rows_with_ab_record_id == probe_transport_rows`
- `probe_entry_rows_with_ab_record_id == probe_entry_rows`
- `join_key_audit = PASS`
- active BUY count remains unchanged by probe rows
- active open position count remains unchanged by probe rows
- probe rows do not appear in `gatekeeper_v2_buys.jsonl`
- probe rows do not set `decision_verdict_buy=true`
- no live/P2 path is enabled

Smoke INCONCLUSIVE:

- no V3/MFS rows,
- no probe-selected rows because deterministic sampler did not select within
  the budget.

Smoke FAIL:

- probe row lacks `ab_record_id`,
- probe row lacks `probe_id`,
- active BUY count changes because of probe rows,
- active open position count changes because of probe rows,
- live sender path is touched,
- audit relies primarily on `pool_mint_time_window`.

## Test Plan

Python checks:

```bash
python3 -m py_compile \
  scripts/shadow_onchain_lifecycle_report.py \
  scripts/v3_p37_shadow_lifecycle_labeler.py \
  scripts/v3_p37_shadow_lifecycle_feature_availability.py \
  scripts/v3_p37_mfs_lifecycle_join_key_audit.py

python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
```

Expected new or expanded Python tests:

```bash
python3 -m unittest scripts/test_v3_p37_counterfactual_shadow_probe_audit.py -v
```

Targeted Rust tests to add:

```bash
cargo test -p ghost-launcher p37_shadow_probe_config_defaults_disabled -- --nocapture
cargo test -p ghost-launcher p37_shadow_probe_enabled_requires_shadow_profile -- --nocapture
cargo test -p ghost-launcher p37_shadow_probe_deterministic_sampler_is_replayable -- --nocapture
cargo test -p ghost-launcher p37_shadow_probe_does_not_mutate_active_verdict -- --nocapture
cargo test -p ghost-launcher p37_shadow_probe_does_not_emit_active_buy_events -- --nocapture
cargo test -p ghost-launcher p37_shadow_probe_backpressure_skips_not_blocks -- --nocapture
cargo test -p ghost-launcher p37_shadow_probe_output_path_collision_fails_closed -- --nocapture
cargo test -p ghost-launcher p37_shadow_probe_transport_entry_join_metadata -- --nocapture
cargo test -p ghost-brain shadow_lifecycle_join_metadata_is_inherited_from_probe_context -- --nocapture
```

Formatting / whitespace:

```bash
rustfmt --edition 2021 --check <touched-rust-files>
git diff --check
```

## Acceptance Criteria

P3.7-J3 P0 is accepted when:

- Config defaults are backward-compatible and disabled.
- Enabled probe config fails closed outside shadow-only execution.
- Deterministic sampler is replayable.
- Eligibility precheck logs explicit skip reasons.
- Every selected/skipped row has `probe_bucket`.
- EventBus is disabled in P0 or emits only dedicated probe events.
- Probe transport rows are generated with `ab_record_id`.
- Probe entry rows are generated with the same `ab_record_id`.
- P0 does not require closed lifecycle positions.
- P1 lifecycle rows inherit the same `ab_record_id` and `probe_id` when
  lifecycle rows exist.
- Probe artifacts include `dispatch_source=counterfactual_shadow_probe`.
- Probe artifacts include amount, quote age, and slippage fields.
- V3/MFS payload is present for probed rows.
- Join-key audit returns PASS for exact AB/probe continuity.
- Legacy rows without probe fields still parse.
- Active verdicts and active BUY counts are not mutated by probe rows.
- Probe rows do not appear in `gatekeeper_v2_buys.jsonl`.
- Probe rows do not set `decision_verdict_buy=true`.
- Probe rows do not alter active `reason_code` / `verdict_type`.
- Probe rows do not increment active BUY metrics.
- Probe rows do not increment active open position count.
- Probe backpressure skips probes instead of blocking active decisions.
- No live/P2/IWIM/threshold path is changed.
- Labels can be generated after lifecycle close in P1.

## Post-P0 Governance

If P0 code/tests pass, run a bounded J3 smoke before any collection run.

If J3 smoke passes, prepare:

```text
PLANS/AUDYT/RAPORT_P3_7_J3_COUNTERFACTUAL_SHADOW_PROBE_SMOKE_202605XX.md
PLANS/AUDYT/RAPORT_P3_7_J3_COUNTERFACTUAL_SHADOW_PROBE_JOIN_KEY_AUDIT_202605XX.md
```

Only after a successful smoke may P3.7 consider a bounded counterfactual probe
collection run.

Even after successful J3 collection, the next step is only:

```text
diagnostic V3/MFS lifecycle feature prototype
```

It is still not:

```text
P2
live
runtime thresholds
selector promotion
```

## Risks

### Counterfactual Selection Bias

Probe labels describe sampled rows, not the active BUY distribution. Reports
must include the sampling frame and selected/skipped counts.

### Misclassification As Active BUY

Every probe artifact must carry `dispatch_source=counterfactual_shadow_probe`.
Reports must segment probe rows from active BUY rows.

### Metadata Gaps

Rows missing AB/V3 hash metadata must be skipped with explicit reasons. They
must not silently fall back to weak joins.

### Runtime Load

Probe dispatch must be bounded by rate, concurrency, and max-probes limits.
Probe backpressure must not block the active decision hot path.

### Label Leakage

Lifecycle and PnL outcomes are labels only. They must never be consumed as
decision-time features.

## Final Gate

P3.7-J3 can unblock dataset collection only after:

- J3 smoke generates probe transport/entry rows with exact AB/probe join keys,
- V3/MFS replay remains strict-clean,
- no active policy mutation is observed,
- join-key audit reports PASS,
- lifecycle labels can be generated after close.

Until then:

```text
Full R14: HOLD
Phase B V3 selector prototype: HOLD
P2/live: NO-GO
```

## P3.7-J3E Probe Payer / Account Resolution

### Trigger

R15-r3 after J3R2 produced strict-clean V3/MFS replay evidence and exact
selection-to-decision hash continuity, but every selected probe was stopped by
the required-account precheck:

```text
missing_required_account:payer_pubkey:HvLVQMA4...
```

No probe transport or entry rows were generated, so collection remains blocked.

### Diagnosis

The payer is not a live or configured wallet in the R15 counterfactual probe
profile. It is the launcher-local shadow payer created by:

```toml
[trigger.shadow_run]
payer_strategy = "ephemeral"
sig_verify = false
replace_recent_blockhash = true
```

The runtime source is `TriggerComponent::load_payer()` and the cached
`cached_shadow_ephemeral_payer`. This payer is intentionally local to the
shadow-only simulation lane and is not expected to be chain-visible. It must not
be treated as live inclusion proof, funded wallet evidence, or active BUY
authorization.

### Precheck Semantics

For counterfactual probe precheck:

- `payer_provenance="ephemeral"` is not a required on-chain execution account.
- Missing ephemeral payer must not stop probe eligibility before simulation.
- `payer_provenance="configured"` remains strict-required.
- True execution accounts remain strict-required: mint, token program, global
  config, fee recipient, creator, associated bonding curve, and user ATA unless
  the prepared request includes an idempotent ATA create.
- If RPC simulation later returns `AccountNotFound`, that remains a real
  simulation/data problem and must be reported. It is not success.

This keeps the probe plane aligned with the existing shadow-only
`payer_strategy="ephemeral"` contract without disabling the account precheck
globally.

### Implementation Scope

P3.7-J3E only changes the counterfactual probe required-account precheck:

- add an explicit allowance for missing `ephemeral` payer,
- preserve strict handling for configured payer,
- preserve strict handling for real execution accounts,
- add targeted tests for both paths,
- create a fresh R15-r4 smoke profile.

### R15-r4 Gate

R15-r4 must use a clean namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r4
```

Acceptance:

- V3/MFS strict replay remains OK.
- Probe selection exact decision/V3 join remains 100%.
- Selected probes no longer all stop on `missing_required_account:payer_pubkey`.
- Any remaining missing account is reported with a precise role/pubkey.
- Probe transport/entry rows are evaluated as runtime evidence only if they
  are actually produced.

Collection, Phase B, P2, live, active policy changes, IWIM changes, and
threshold tuning remain out of scope.

## P3.7-J3F Probe Required Transaction Account Resolution

### Trigger

R15-r4 confirmed that the J3E payer repair worked:

```text
missing_required_account:payer_pubkey no longer blocks selected probes
```

The next blocker was:

```text
missing_required_account:transaction_account:4NpkpkjPC9DYD2nSLsmLWKBsLXEPgZSkXySpwUoMgiLL
```

for all selected probes. No probe transport or entry rows were generated.

### Diagnosis

The missing account is not a generic unknown account from the V3 row. It is
introduced by the routed `DirectBuyBuilder::build_buy_ix_with_accounts(...)`
instruction. The account has the same pubkey across selected probes because it
is derived from the stable ephemeral shadow payer, not from the mint.

The routed pump.fun `buy_exact_sol_in` account layout includes:

```text
account[12] = global_volume_accumulator
account[13] = user_volume_accumulator
account[14] = fee_config
account[15] = fee_program
account[16] = bonding_curve_v2
account[17] = buyback_fee_recipient
```

J3F resolves the generic `transaction_account` role by mapping routed buy
instruction account positions into explicit roles. The missing account from
R15-r4 maps to `user_volume_accumulator`.

### Precheck Semantics

For counterfactual probe precheck:

- `user_volume_accumulator` is a per-payer routed pump.fun volume PDA.
- Missing `user_volume_accumulator` should not stop probe eligibility before
  simulation, because the probe purpose is to learn whether the exact prepared
  transaction can simulate under current runtime conditions.
- Missing `user_volume_accumulator` is not success. If simulation still returns
  AccountNotFound, it remains a simulation/data problem and must be reported.
- Other routed buy accounts remain strict unless separately justified: mint,
  token program, global config, fee recipient, bonding curve, associated bonding
  curve, creator vault, event authority, pump program, global volume
  accumulator, fee config, fee program, bonding_curve_v2 and buyback fee
  recipient.

### Implementation Scope

P3.7-J3F only changes counterfactual probe account role resolution:

- map known `DirectBuyBuilder` routed buy account indices to explicit roles;
- allow missing `user_volume_accumulator` through precheck for routed probe
  requests only;
- keep strict handling for true required execution accounts;
- add targeted tests for the role mapping and strict-account preservation;
- create a fresh R15-r5 smoke profile.

### R15-r5 Gate

R15-r5 must use a clean namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r5
```

Acceptance:

- V3/MFS strict replay remains OK.
- Probe selection exact decision/V3 join remains 100%.
- Selected probes no longer all stop on
  `missing_required_account:user_volume_accumulator` or generic
  `transaction_account` for the known routed user-volume PDA.
- Any remaining missing account is reported with a precise role/pubkey.
- Probe transport/entry rows are evaluated as runtime evidence only if they
  are actually produced.

Collection, Phase B, P2, live, active policy changes, IWIM changes, and
threshold tuning remain out of scope.

### R15-r5 Result

R15-r5 was run as a bounded smoke after J3F.

Observed result:

```text
v3_rows = 169
strict replay status = full_replay_ok
strict replay bad_rows = 0
probe_selection_rows = 5
probe_transport_rows = 0
probe_entry_rows = 0
active_shadow_transport_rows = 0
active_shadow_entry_rows = 0
```

J3F fixed the R15-r4 blocker: no selected probe was stopped by the routed
`user_volume_accumulator` account and no selected probe reported a generic
`transaction_account` blocker for the known user-volume PDA.

The runtime gate remains not ready. The five selected probes were stopped by
strict precheck failures on true routed execution accounts:

```text
missing_required_account:bonding_curve_v2 = 4
missing_required_account:creator_vault = 1
```

Decision/V3 hash continuity for probe selection remained clean:

```text
probe_selection exact decision/V3 join = 5/5
feature_hash_mismatch = 0
policy_hash_mismatch = 0
```

Decision:

```text
P3.7-J3F code-level repair: PASS
R15-r5 runtime smoke: NOT_READY_DIAGNOSED
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

The next implementation stage should not weaken required-account precheck.
It should decide how counterfactual probes obtain or wait for strict routed
execution accounts such as `bonding_curve_v2` and `creator_vault`, while
preserving decision-time safety and exact join-key continuity.

## P3.7-J3G Probe Strict Execution Account Readiness

### Trigger

R15-r5 confirmed that J3F removed the payer, routed user-volume, and generic
`transaction_account` blockers. The remaining selected-probe blockers were
strict execution accounts:

```text
missing_required_account:bonding_curve_v2 = 4
missing_required_account:creator_vault = 1
```

No probe transport or entry rows were generated, so collection remained
blocked.

### Audit Scope

J3G adds an offline readiness audit:

```text
scripts/v3_p37_probe_execution_account_readiness_report.py
```

The audit correlates:

- `probe_selection.jsonl`,
- `probe_skips.jsonl`,
- persisted Gatekeeper/V3 decision rows,
- V3/MFS snapshots,
- system/oracle logs for required-account update evidence.

The audit is read-only. It does not dispatch probes, bypass precheck, change
sampling, or touch active policy.

### R15-r5 Account Readiness Result

J3G diagnosed all five selected R15-r5 probes:

```text
selected_probe_rows = 5
diagnosed_selected_probe_rows = 5
exact_decision_v3_join_rows = 5
missing_account_roles = {"bonding_curve_v2": 4, "creator_vault": 1}
classification = override_present_but_account_missing_on_rpc for 5/5
```

Interpretation:

- the required pubkeys were present in the prepared transaction account set;
- processed RPC/precheck did not find those accounts;
- no `DIAG_ACCOUNT_UPDATE_RELAY` evidence was found for the required pubkeys;
- the source decision rows had V3/MFS snapshots with curve/account evidence
  marked clean/ready;
- the MFS snapshots do not explicitly materialize `bonding_curve_v2` or
  `creator_vault` account identities/readiness.

This is a diagnosed execution-account readiness gap, not a join-key or payer
problem.

### Decision

```text
P3.7-J3G account readiness audit: PASS
R15-r5 runtime smoke: NOT_READY_DIAGNOSED
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

Next stage:

```text
P3.7-J3H Probe Execution-Account Eligibility
```

J3H must choose a concrete decision-time-safe fix before R15-r6:

- add explicit additive materialization/readiness for strict execution accounts,
- or restrict probe eligibility to rows with known execution-account readiness,
- or add a bounded decision-time-safe wait for those accounts,
- or repair a proven account override/build path mismatch.

J3H must not weaken strict precheck, use post-hoc account guessing, increase
probe limits, or treat missing core execution accounts as success.

## P3.7-J3H Probe Execution-Account Eligibility

### Trigger

J3G established that the remaining R15-r5 blockers are true routed execution
accounts, not payer, user-volume, generic transaction-account, or hash-join
issues:

```text
bonding_curve_v2 = 4
creator_vault = 1
classification = override_present_but_account_missing_on_rpc
```

The selected decision rows had exact V3/MFS joins, but the required account
identities/readiness were not explicit in the materialized decision snapshot.

### Implementation

J3H keeps strict required-account precheck intact and changes only probe-plane
classification:

- strict execution account roles are enumerated explicitly;
- `bonding_curve_v2` is treated as a core strict execution account;
- `creator_vault` is treated as route-aware because the role is assigned from
  the routed buy instruction account layout;
- if a strict execution account is missing at processed precheck, the selected
  probe is skipped with:

```text
probe_skip_reason = execution_account_not_ready
precheck_failure_reason = execution_account_not_ready:<role>:<pubkey>
execution_account_readiness_status = not_ready
execution_account_readiness_role = <role>
execution_account_readiness_pubkey = <pubkey>
```

This is not a bypass. The probe still does not dispatch when a strict execution
account is unavailable. The difference is that R15-r6 and later audits can
distinguish execution-account readiness from generic precheck failures.

### R15-r6 Gate

R15-r6 must use a clean bounded smoke namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r6
```

Acceptance:

- V3/MFS strict replay remains OK.
- Probe selection exact decision/V3 join remains 100%.
- Selected probes have explicit `execution_account_readiness_status`.
- Missing `bonding_curve_v2` or route-specific `creator_vault` is reported as
  `execution_account_not_ready`, not as an ambiguous missing account.
- If all selected probes are skipped, the run is `NOT_READY_DIAGNOSED`, not a
  collection-ready PASS.
- Collection, Phase B, P2, live, active policy changes, IWIM changes and
  threshold tuning remain out of scope.

### Next Decision After R15-r6

If R15-r6 reaches probe entries, proceed only to a small bounded probe
collection gate. If R15-r6 produces `execution_account_not_ready` skips, the
next repair must be one of:

- additive decision-time materialization/readiness for routed strict accounts,
- route-aware eligibility requiring strict execution-account readiness,
- or a bounded decision-time-safe wait for account readiness.

Do not weaken strict precheck and do not infer missing account readiness from
post-hoc data.

## P3.7-J3I Probe Execution-Account Eligibility Narrowing

### Trigger

R15-r6 produced V3/MFS rows and strict replay OK, and selected probe rows still
exact-joined to persisted V3 decision rows. All selected probes were stopped by:

```text
execution_account_not_ready:bonding_curve_v2:<pubkey>
```

This confirmed that J3H semantics are correct, but it also exposed a quota
semantics bug: selected but execution-account-not-ready rows consumed the
`max_probes_per_run` dispatch budget before the probe plane knew whether the row
was execution-ready.

### Decision

J3I separates candidate scan/diagnosis from dispatch quota:

- selected probe candidates may reserve a bounded scan slot and run eligibility
  diagnostics;
- `execution_account_not_ready` rows are logged and counted as skips;
- not-ready rows do not increment `max_probes_per_run`;
- `max_probes_per_run`, `max_probes_per_minute`, and dispatch concurrency are
  consumed only after required execution-account readiness is proven;
- `bonding_curve_v2` remains a strict core execution account;
- `creator_vault` remains route-aware and strict only when required by the
  routed request layout.

This is not a precheck bypass. Strict required-account readiness remains the
gate before shadow simulation. J3I only prevents non-dispatchable rows from
exhausting the dispatch probe budget.

### Runtime Shape

The probe runtime state is split into:

```text
scan reservation:
  dedupe probe_id
  bounded scan concurrency
  no dispatch quota consumption

dispatch reservation:
  max_probes_per_run
  max_probes_per_minute
  dispatch concurrency
  only after execution-account readiness = ready
```

If a selected row is not execution-ready, it is written as:

```text
event_type = probe_skipped
probe_skip_reason = execution_account_not_ready
precheck_failure_reason = execution_account_not_ready:<role>:<pubkey>
execution_account_readiness_status = not_ready
```

If a row becomes execution-ready but dispatch budget is exhausted, it is written
as a normal probe skip with `max_probes_per_run_exceeded`,
`probe_rate_limit_exceeded`, or `probe_concurrency_limit_exceeded`.

### R15-r7 Gate

R15-r7 must use a clean bounded smoke namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r7
```

Acceptance:

- V3/MFS strict replay remains OK.
- Probe selection exact decision/V3 join remains 100%.
- Not-ready rows do not consume dispatch quota.
- Dispatch quota is consumed only by execution-ready rows.
- If execution-ready rows exist, the smoke reaches probe transport/entry or
  reports a precise simulation/build failure.
- If no execution-ready rows exist in the scanned candidate universe, the run is
  `NOT_READY_DIAGNOSED`, not a dispatch-quota failure.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

## P3.7-L1 Diagnostic Standard / Soft-PDD Policy Probe

### Context

J4C proved the counterfactual evidence chain end to end:

```text
V3/MFS decision artifact
-> counterfactual probe selection/transport
-> shadow entry
-> probe lifecycle close
-> on-chain lifecycle report
-> lifecycle labeler
-> feature availability join
```

The J4C dataset was technically usable but selection-useless: all lifecycle
labels were `buy_quality_bad` / `market_bad_clean`, with no `good` or
`dirty_good` rows. The active J4C brain config was the defensive rollout config
`ghost_brain_v3_p37_mfs_lifecycle.toml`, combining long-mode final evaluation,
static/tight PDD drift, hard spike/ramping vetoes, tight HHI hard fail, and the
Prosperity filter.

L1 is therefore a diagnostic policy probe, not a candidate live policy.

### Scope

L1 keeps the J4C baseline immutable and adds a separate R16 namespace/config
family:

```text
configs/rollout/ghost_brain_v3_p37_l1_standard_softpdd.toml
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1.toml
```

R16 is a bundle-relaxation screening run, not a clean ablation. It changes the
policy bundle only inside the diagnostic rollout:

```text
gatekeeper_v2.mode = "standard"
gatekeeper_v2.max_wait_time_ms = 5000
gatekeeper_v2.dow.extended_window_ms = 5000
gatekeeper_v2.dow.extended_require_pdd_clean = false
gatekeeper_v2.pdd.spike_hard_veto = false
gatekeeper_v2.pdd.ramping_hard_veto = false
gatekeeper_v2.pdd.entry_drift_soft_max_pct = 8.0
gatekeeper_v2.pdd.entry_drift_max_pct = 15.0
gatekeeper_v2.pdd.entry_drift_elapsed_scaling_enabled = true
gatekeeper_v2.pdd.entry_drift_elapsed_base_pct = 6.0
gatekeeper_v2.pdd.entry_drift_elapsed_slope_pct_per_second = 1.8
gatekeeper_v2.pdd.entry_drift_elapsed_cap_pct = 15.0
gatekeeper_v2.hard_fail_hhi = 0.20
gatekeeper_v2.hard_fail_top3_volume_pct = 0.95
gatekeeper_v2.enable_prosperity_filter = false
```

R16 deliberately leaves these baseline gates unchanged for first-pass
diagnostics:

```text
gatekeeper_v2.max_hhi = 0.155
gatekeeper_v2.min_bonding_progress_pct = 40.0
gatekeeper_v2.min_market_cap_sol = 41.0
gatekeeper_v2.min_tx_count
gatekeeper_v2.min_unique_signers
gatekeeper_v2.alpha gate
```

If R16 produces any `good` / `dirty_good` labels, later work must ablate one
axis at a time. R16 alone may show that the J4C baseline is too defensive, but
it does not identify the single responsible threshold.

### Diagnostics Added

Decision rows gain additive schema-v21 diagnostics:

```text
pdd_entry_drift_elapsed_ms
pdd_entry_drift_anchor_price
pdd_entry_drift_current_price
pdd_entry_drift_anchor_ts_ms
pdd_entry_drift_current_ts_ms
pdd_entry_drift_static_max_pct
pdd_entry_drift_elapsed_max_pct
pdd_entry_drift_effective_max_pct
pdd_entry_drift_threshold_source
pdd_spike_ratio
pdd_spike_ratio_quality
pdd_spike_recent_rate
pdd_spike_earlier_rate
pdd_whale_single_max_pct
gatekeeper_first_kill_gate
gatekeeper_first_kill_reason
gatekeeper_terminal_gate
gatekeeper_gate_trace
```

PDD diagnostics must come from the same PDD evaluation/anchor choice used by
the policy decision. DecisionLogger only persists these fields; it must not
recompute first-kill gate or PDD anchor metrics after the fact.

`pdd_spike_ratio_quality` is explicit:

```text
ok
earlier_rate_zero
insufficient_earlier_window
insufficient_recent_window
unavailable
```

When `earlier_rate = 0`, `pdd_spike_ratio` is not emitted as infinity. The
ratio is `null` and quality is `earlier_rate_zero`.

Every drift decision logs both static and elapsed-aware thresholds plus the
effective threshold actually used. `PDD drift rows` means rows where the PDD
entry-drift gate was evaluated, regardless of whether drift was the terminal
reason.

### R16 Identity Contract

Every R16 decision/probe/lifecycle row must carry:

```text
R16 namespace
run_id
session_id
brain_config_path
brain_config_hash
v3_policy_config_hash
```

R16 artifacts must not mix J4C decision rows with R16 probe/lifecycle rows. The
L1 diagnostics report must show `v3_policy_config_hash` and `brain_config_hash`
distributions and confirm one active hash for R16.

R16 also validates BUY lifecycle coverage explicitly:

```text
R16 BUY verdict count
R16 BUY shadow entry count
R16 BUY lifecycle close count
R16 REJECT/PENDING probe lifecycle count
```

The normal shadow execution path is the preferred lifecycle source for R16 BUY
verdicts. Because this is diagnostic-only, R16 also includes BUY rows in the
counterfactual probe plane as a fallback to ensure BUY outcomes are lifecycle
labelable without changing live or active policy semantics.

### Reporting

The L1 report script is:

```text
scripts/v3_p37_l1_reject_diagnostics.py
```

It writes:

```text
logs/shadow_run/<r16-namespace>/p3_7_l1_per_reject_diagnostics.jsonl
logs/shadow_run/<r16-namespace>/p3_7_l1_reject_diagnostics_summary.json
logs/shadow_run/<r16-namespace>/p3_7_l1_reject_diagnostics_summary.md
```

If R16 still has `0 good` and `0 dirty_good`, the report must show distributions
for the gates deliberately left at baseline:

```text
max_hhi
min_bonding_progress_pct
min_market_cap_sol
min_tx_count
min_unique_signers
alpha gate
```

The report is invalid for policy conclusions unless diagnostic coverage passes:

```text
pdd_entry_drift_elapsed_ms/anchor/current price coverage >= 95% on PDD drift rows
pdd_spike_ratio_quality populated on >= 95% spike-diagnostic rows
pdd_spike_ratio populated on >= 95% rows where ratio quality is ok
pdd_whale_single_max_pct populated on >= 95% whale-diagnostic rows
gatekeeper_first_kill_gate or gatekeeper_terminal_gate populated on >= 95% terminal rejects/timeouts
```

### Entry Wait Check

`entry_wait_ms` appears in rollout configs as post-BOUGHT stabilization
configuration. The L1 implementation must verify read-only that it does not
delay pre-entry/probe dispatch. If later code proves it affects pre-entry or
probe dispatch, R16 must not start without explicit `entry_wait_applied_ms` and
a blocker classification.

### R16 Gate

R16 uses a small bounded lifecycle-label probe run:

```text
max_probes_per_run = 50
max_concurrent = 1
max_probe_candidates_scanned_per_run = 20000
```

Non-goals remain:

```text
no Phase B
no P2/live
no active policy promotion
no root ghost_brain_config.toml edit
no baseline J4C config edit
```

## P3.7-L1R Diagnostic Hydration Repair

### Trigger

R16 standard/soft-PDD r1 produced useful runtime evidence:

```text
strict replay PASS
probe lifecycle labels included dirty_good rows
active shadow BUY lifecycle labels included dirty_good rows
```

But it was not a valid policy diagnostic PASS because the fields intended to
explain the reject universe were not hydrated:

```text
pdd_entry_drift_anchor_coverage_pct = 0.0
pdd_spike_ratio_quality_coverage_pct = 0.0
whale_single_max_pct_coverage_pct = 0.0
r16_artifact_identity_status = FAIL
single_active_hash_status = FAIL
```

L1R repairs diagnostic hydration and artifact identity only. It does not change
policy thresholds and does not start ablation.

### Scope

L1R hydrates materialized PDD diagnostics in the same policy/evaluation path
used by R16 decision rows:

```text
pdd_entry_drift_elapsed_ms
pdd_entry_drift_anchor_price
pdd_entry_drift_current_price
pdd_entry_drift_anchor_ts_ms
pdd_entry_drift_current_ts_ms
pdd_entry_drift_elapsed_max_pct
pdd_entry_drift_effective_max_pct
pdd_entry_drift_threshold_source
pdd_spike_ratio
pdd_spike_ratio_quality
pdd_spike_recent_rate
pdd_spike_earlier_rate
pdd_whale_single_max_pct
```

The materialized path must preserve the same source hierarchy used for PDD
decisions. It may derive diagnostic anchor price from the materialized current
price and drift percentage only when both values are finite and positive. If
anchor/current timestamps or prices are missing, invalid, or unordered, the row
must be explicitly degraded with a threshold source such as
`fallback_no_anchor` or `invalid_timestamp_order`; it must not emit NaN, inf, or
synthetic confidence.

For spike diagnostics, `pdd_spike_ratio_quality` remains explicit:

```text
ok
earlier_rate_zero
insufficient_earlier_window
insufficient_recent_window
unavailable
```

When `earlier_rate = 0`, the ratio is `null` and quality is
`earlier_rate_zero`; infinite ratios are not serialized.

For whale diagnostics, Path B materialized features do not carry full signer
volume attribution. L1R therefore emits a decision-time-safe
`pdd_whale_single_max_pct` from the materialized maximum transaction share of
total volume. This is a diagnostic proxy for R16 reporting, not a replacement
for richer signer-level whale attribution.

### Identity Repair

Shadow dispatch failure rows must inherit the same `ExecutionJoinMetadata` as
the decision/entry path. Even failed active-shadow lifecycle rows must carry:

```text
run_id
session_id
rollout_namespace
brain_config_hash
v3_policy_config_hash
ab_record_id, when available
```

Success-only stamping is not sufficient; failure rows participate in the R16
artifact identity contract and must not break single-hash accounting.

### R16-r2 Gate

After code-level L1R repair, rerun the same R16 policy bundle in a fresh
namespace:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r2.toml
```

The R16-r2 policy thresholds must match R16-r1. The run validates only
diagnostic hydration and artifact identity.

L1R runtime acceptance:

```text
strict replay = full_replay_ok
diagnostic_quality.status = PASS
pdd_entry_drift_anchor_coverage_pct >= 95%
pdd_spike_ratio_quality_coverage_pct >= 95% on spike diagnostic rows
whale_single_max_pct_coverage_pct >= 95% on whale diagnostic rows
gatekeeper_first_or_terminal_gate_coverage_pct = 100%
r16_artifact_identity_status = PASS
single_active_hash_status = PASS
custom_2006 classified, not unknown
active BUY/probe lifecycle labels reported separately
```

If R16-r2 has diagnostic PASS and still produces `good` or `dirty_good` rows,
the next step is `P3.7-L2 Policy Axis Ablation`. If diagnostics still fail,
repair logger/PDD propagation again. If dirty-good disappears, repeat the same
bounded R16 config once before drawing policy conclusions.

Non-goals remain:

```text
no ablation
no Phase B
no P2/live
no threshold tuning
no root ghost_brain_config.toml edit
no baseline J4C config edit
```

## P3.7-J4B Probe Lifecycle Truth Resolution / Runtime Retention

### Trigger

J4 validated the counterfactual probe lifecycle handoff:

```text
probe_shadow_entry -> PostBuySubmitted(lane=probe) -> probe monitor -> probe_shadow_lifecycle
```

The J4 bounded run produced probe lifecycle rows with exact `ab_record_id`,
`probe_id`, and V3 hash continuity, but every probe lifecycle row closed as:

```text
truth_status = failure
truth_detail = shadow time-stop expired before any canonical snapshot reached guardian
```

The handoff was therefore fixed, but economic truth resolution was not yet
validated.

### Diagnosis

The failure is not a Gatekeeper decision-rate problem and not a probe
transport/entry problem. Runtime logs showed the following sequence for probe
rows:

1. `DIAG_ACCOUNT_UPDATE_RELAY` and `DIAG_ACCOUNT_UPDATE_APPLIED` existed before
   the decision.
2. The pool reached a terminal REJECT/TIMEOUT decision.
3. `pool_task_done_cleanup` removed `AccountStateCore`, `ShadowLedger`
   snapshots, curve aliases, live-pipeline mint state, and pending account
   updates for the base mint.
4. Counterfactual probe dispatch and `PostBuySubmitted(lane=probe)` happened
   asynchronously after that cleanup.
5. The probe monitor accepted the position but timed out without any canonical
   snapshot reaching the guardian.

That means the probe monitor was correctly registered, but the runtime truth
state it depends on was removed before the probe lifecycle could use it.

### Repair

J4B makes counterfactual probe dispatch request runtime retention for the pool.
When a probe row passes selection/precheck and reserves a scan slot for
background dispatch, `maybe_handle_p37_shadow_probe_decision` returns a
retention flag to the pool observation result. The pool router then treats the
terminal decision as:

```text
retain_runtime_pool = true
```

for that pool, preventing `pool_task_done_cleanup` from deleting canonical
runtime truth while the probe lifecycle monitor is active.

The repair is intentionally narrow:

- no active verdict changes;
- no Gatekeeper threshold changes;
- no IWIM changes;
- no P2/live changes;
- no lifecycle fallback to synthetic or stale shadow prices;
- no bypass of canonical snapshot requirements.

### Validation Gate

The next bounded smoke must use a fresh namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1
```

Acceptance:

- V3/MFS strict replay remains OK.
- Probe selection/transport/entry/lifecycle exact decision/V3 join remains
  100%.
- Probe lifecycle rows are still emitted only under
  `dispatch_source=counterfactual_shadow_probe`.
- Probe positions no longer close only because no canonical snapshot ever
  reached the guardian.
- If lifecycle truth remains `failure`, the report must distinguish
  no-snapshot, stale snapshot, unnormalizable price, and exit-truth failures.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope until J4B runtime evidence is reviewed.

## P3.7-J4C Probe On-Chain Lifecycle Report / Label Repair

### Trigger

J4B validated probe runtime retention and produced resolved lifecycle truth:

```text
probe_transport_rows = 25
probe_shadow_entry_rows = 25
probe_shadow_lifecycle_rows = 48
truth_status = resolved
truth_source = canonical_account_state_snapshot
```

However the post-run reporting gate failed before labels could be generated:

```text
NameError: name 'lifecycle' is not defined
```

The failure was in `scripts/shadow_onchain_lifecycle_report.py`, not in the
runtime probe lifecycle. The report also only knew the active shadow artifact
paths by default, while P3.7 probe rows live under `[p37_shadow_probe]`.

### Repair

J4C makes the reporting path probe-compatible while preserving the existing
shadow default:

- add `--artifact-plane {shadow,probe}` and `--probe`;
- when `--probe` is used, read `[p37_shadow_probe]` transport, entry and
  lifecycle paths;
- preserve `probe_id`, `dispatch_source`, `source_ab_record_id`, `run_id` and
  `session_id` in report rows additively;
- fix the undefined lifecycle variable by coalescing join metadata from the
  current lifecycle bundle;
- allow the P3.7 lifecycle labeler to classify
  `counterfactual_shadow_probe_simulated` as a valid shadow/probe simulated
  execution outcome without treating it as an active BUY.

### J4B Artifact Revalidation

J4C reuses the already collected J4B namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1
```

Acceptance:

- probe on-chain lifecycle report writes rows successfully;
- report rows carry exact `ab_record_id`, `probe_id`, dispatch source and V3
  hashes;
- lifecycle labels are generated from probe rows;
- feature availability joins lifecycle labels to V3/MFS decision rows by exact
  `ab_record_id`;
- Phase B remains blocked unless sample size and class balance meet explicit
  selector-readiness minimums.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope until J4C reporting and feature availability results
are reviewed.

## P3.7-J4 Probe Lifecycle Handoff / Post-Buy Monitor Validation

### Trigger

J3L-r1 validated bounded counterfactual probe transport and entry generation:

```text
probe_transport_rows = 25
probe_shadow_entry_rows = 25
exact decision/V3 join = 100%
active_buys_rows = 0
probe_lifecycle_rows = 0
```

The blocker moved from selection/dispatch/entry to the next boundary:

```text
probe entry -> lifecycle handoff -> post-buy monitor
```

Decision thresholds, active V2/V2.5 policy, IWIM, live/P2 and ghost brain
thresholds are not implicated by this failure class and must remain unchanged.

### Root Cause

The J3 counterfactual probe dispatch path wrote `probe_transport.jsonl` and
`probe_shadow_entries.jsonl`, but did not send a `PostBuySubmitted` handoff to
`PostBuyRuntime`. The canonical shadow lifecycle path only starts when
`PostBuyRuntime` receives a handoff and registers the position in
`MonitoringEngine`.

The `probe_position_id` on the entry row was only serialized metadata. It did
not create a monitored position.

### Repair Contract

J4 adds a probe-only lifecycle handoff:

- successful probe simulation + entry materialization sends a direct-only
  `PostBuySubmitted` event with `lane="probe"`;
- `PostBuyRuntime` owns a separate probe `MonitoringEngine`;
- the probe monitor uses an isolated `ShadowPositionBook`;
- probe lifecycle proof writes to `p37_shadow_probe.lifecycle_log_path`;
- canonical shadow lifecycle path remains separate;
- no active position slot is reserved;
- no live sender path is reachable;
- active BUY counters/logs remain untouched;
- join metadata (`ab_record_id`, `probe_id`, V3 hashes) is inherited by the
  probe monitored position.

### Acceptance

J4 code-level repair is accepted when:

- probe lane handoff registers a position in the probe monitor;
- the canonical shadow monitor remains untouched for probe handoffs;
- probe lifecycle rows inherit `ab_record_id` / `probe_id` / V3 hashes when a
  probe position closes;
- probe lifecycle writes to the probe lifecycle path, not active shadow path;
- existing P3.7 probe tests still pass;
- no active policy, IWIM, thresholds, P2 or live configuration changes are made.

### Next Runtime Gate

Run a fresh bounded J4 namespace before claiming runtime lifecycle validation.
The next smoke must prove:

```text
probe_transport_rows > 0
probe_shadow_entry_rows > 0
probe_lifecycle_monitor_started > 0
active_buys_rows = 0
exact decision/V3 join = 100%
no live/P2 path touched
```

Lifecycle close / on-chain labels remain a later gate unless the bounded run
naturally produces closed probe positions.

### R15 Bounded J4-r1 Result

Fresh namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4-r1
```

Observed result:

```text
v3_rows = 236
strict replay = full_replay_ok 236/236
probe_selection_rows = 92
probe_transport_rows = 25
probe_shadow_entry_rows = 25
probe_shadow_lifecycle_rows = 50
probe_lifecycle_exit_blocked_rows = 25
probe_lifecycle_position_closed_rows = 25
probe_lifecycle_truth_status_failure_rows = 50
probe_lifecycle_time_stop_rows = 25
probe exact decision/V3 join = 100%
probe transport/entry/lifecycle ab_record_id coverage = 100%
probe transport/entry/lifecycle probe_id coverage = 100%
```

The J4 lifecycle handoff boundary is runtime-validated:

```text
probe_shadow_entry -> PostBuySubmitted(lane=probe) -> probe monitor -> probe_shadow_lifecycle
```

The run also produced natural non-probe shadow artifacts:

```text
buys.jsonl rows = 2
shadow_lifecycle.jsonl rows = 6
```

Those rows do not carry `probe_id` and are separate from
`dispatch_source=counterfactual_shadow_probe` probe artifacts. They do not
indicate probe mutation of active BUY semantics.

Remaining blocker:

```text
probe lifecycle rows are all truth_status=failure
truth_detail = shadow time-stop expired before any canonical snapshot reached guardian
```

Decision:

```text
J4 probe lifecycle handoff = PASS
probe lifecycle economic labels = NOT_READY
full collection / Phase B / P2 / live / threshold tuning = HOLD / NO-GO
```

The next narrow gate is probe lifecycle truth resolution / canonical snapshot
coverage, not decision threshold tuning or larger collection.

## P3.7-J3K7 Routed Exact-SOL-In Entry Materialization / Dispatch Eligibility

### Trigger

R15 J3K6-r1 proved that the creator-vault authority guard works: rows with
non-authoritative creator-vault identity are now written as
`probe_skipped` before simulation instead of reaching Pump.fun as known-bad
`custom_2006` candidates.

The same smoke exposed the next narrower blocker:

```text
probe_transport_rows = 10
probe_entry_rows = 0
buy_variant = routed_exact_sol_in
token_param_role = min_tokens_out
entry_token_amount_raw = null
probe_entry_materialization_status = transport_only_missing_token_quantity
```

This means the probe path was still able to consume dispatch quota and produce
transport rows without enough token-quantity evidence to materialize an entry.
That is not a collection-ready state.

### Decision

J3K7 tightens the probe-only routed exact-SOL-in path:

- populate `legacy_buy_curve` for `RoutedExactSolIn` from decision-time-safe
  route/account evidence when it is available;
- let routed exact-SOL-in request preparation compute
  `entry_token_amount_raw` from that curve snapshot;
- copy the simulation-derived `entry_token_amount_raw` into probe transport
  rows when the request did not already carry a request-time quantity;
- fail closed with `missing_routed_entry_quote_curve` before dispatch when the
  routed row has no decision-time-safe curve snapshot for entry quantity
  derivation.

The repair is counterfactual-only. It does not change active verdicts, active
BUY behavior, IWIM, live sender behavior, thresholds, P2, or selector logic.

### Required Behavior

For `RoutedExactSolIn` probe candidates:

```text
if decision-time-safe curve snapshot exists:
  prepare request with routed exact-SOL-in token quantity evidence
  simulate counterfactual shadow probe
  materialize entry when simulation returns token quantity

if no curve snapshot exists:
  write probe_skipped
  probe_skip_reason = probe_execution_precheck_failed
  precheck_failure_reason = missing_routed_entry_quote_curve
  do not consume dispatch quota
  do not emit transport-only rows
```

Probe transport must prefer the simulation report token quantity over a missing
request token quantity:

```text
entry_token_amount_raw = event.entry_token_amount_raw
  or request.entry_token_amount_raw
```

This keeps transport rows aligned with the actual simulation result.

### R15 J3K7 Gate

Use a clean bounded namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k7-r1
```

Minimal PASS:

- strict V3 replay OK;
- probe selection/transport/entry exact decision/V3 join remains 100%;
- creator-vault non-authoritative rows remain precheck skips;
- routed exact-SOL-in rows either have `entry_token_amount_raw` and entry rows,
  or are skipped before dispatch as `missing_routed_entry_quote_curve`;
- `transport_only_missing_token_quantity` does not dominate dispatched rows;
- active BUY rows remain zero;
- live/P2 paths remain untouched.

If J3K7 still produces mostly transport-only rows with missing token quantity,
collection remains blocked and the next repair must be in routed token-amount
derivation or route identity. Do not increase dispatch scale to hide this.

## P3.7-J3K6 Creator Vault Authority / Route Identity Repair

### Trigger

R15 J3K5-r2 validated the counterfactual probe transport/entry path, but two
probe rows failed simulation with:

```text
simulation_account_layout_mismatch:custom_2006
creator_vault_authority_status = creator_vault_source_not_authoritative
creator_identity_authoritative = false
```

Those rows proved that `custom_2006` is no longer a hash, payer, bonding-curve
or token-parameter plumbing issue. It is a route/account identity issue: the
request reached Pump.fun, but the creator vault passed in the prepared buy did
not satisfy the program's creator-vault seed constraint.

### Decision

J3K6 changes the probe-only path from "simulate and classify this known bad
creator-vault source" to "skip before simulation when the route requires a
creator vault and the creator identity source is not authoritative".

The repair is additive and counterfactual-only:

- selection/skip rows now carry creator-vault authority fields;
- route-aware precheck runs before probe dispatch/reservation;
- non-authoritative creator identity produces
  `probe_skip_reason = creator_vault_source_not_authoritative`;
- strict execution-account precheck remains in place for true required accounts;
- active verdicts, active BUY logs, IWIM, live sender and thresholds are not
  changed.

The first J3K6 conservative rule is:

```text
LegacyBuy:
  detected_pool.creator is not authoritative for creator_vault derivation.

RoutedExactSolIn:
  detected_pool.creator can be used as route-scoped creator identity when
  routed account identity is otherwise complete.
```

This avoids sending the known non-authoritative LegacyBuy creator-vault class
into `simulate_buy`. It may reduce probe entry yield until a stronger
authoritative LegacyBuy creator source is materialized; that is an intentional
fail-closed tradeoff for this stage.

### Required Fields

New skip rows may include:

```text
route_requires_creator_vault
creator_vault_requirement_source
creator_vault_authority_status
creator_vault_mismatch_reason
creator_identity_source
creator_identity_authoritative
```

The join-key audit reports creator-vault authority counts for both transport
simulation-error rows and skip rows, so a J3K6 smoke can distinguish:

```text
custom_2006 still reached simulation
vs.
creator_vault_source_not_authoritative skipped before simulation
```

### R15 J3K6 Gate

Use a clean bounded namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k6-r1
```

Minimal PASS:

- strict V3 replay OK;
- probe exact decision/V3 join remains 100%;
- probe transport/entry rows exist for authoritative/ready routes;
- `custom_2006 = 0` or every remaining `custom_2006` has a new explicit
  creator-vault authority reason not covered by J3K6;
- non-authoritative creator-vault rows appear as `probe_skipped`, not
  simulation-success rows;
- active BUY rows remain zero;
- live/P2 paths remain untouched.

If `creator_vault_source_not_authoritative` dominates skips and entry yield
collapses, the next step is not collection. It is to add or identify an
authoritative LegacyBuy creator source, not to bypass the precheck.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

## P3.7-J3K5 Creator-Vault Source Authority / Amount Guard

### Trigger

J3K4 bounded R1 produced counterfactual probe transport/entry rows with exact
V3 decision join, but left two simulation classes that must be understood before
scaling:

```text
simulation_account_layout_mismatch:custom_2006 = creator_vault actual/expected mismatch
simulation_slippage_or_price_mismatch:custom_6002 = TooMuchSolRequired
```

The creator-vault case is especially sensitive because the expected vault
appears only in Anchor error diagnostics. That value is post-simulation
evidence and must not be used to silently rebuild or correct probe requests.

### Decision

J3K5 keeps request construction unchanged and makes both classes auditable:

- creator-vault mismatch rows carry `creator_vault_authority_status`,
  `creator_vault_actual_pubkey`, `creator_vault_expected_pubkey`,
  `creator_vault_mismatch_reason`, `creator_identity_source` and
  `creator_identity_authoritative`;
- `creator_vault_source_not_authoritative` means the probe request's creator
  source derived a vault that does not match the program-expected vault;
- expected creator-vault values parsed from logs are diagnostic-only and must
  not be fed back into request construction;
- Pump.fun `TooMuchSolRequired` rows carry
  `amount_provided_lamports_if_available`,
  `amount_required_lamports_if_available`,
  `amount_shortfall_lamports_if_available` and `amount_guard_status`;
- join-key audit reports creator-vault authority, mismatch reason, identity
  source, custom error code and amount guard counts.

### R15 J3K5 Gate

The next bounded run, if executed, uses:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k5-r1.toml
```

Acceptance:

- strict V3/MFS replay remains OK;
- probe selection/transport/entry exact decision join remains 100%;
- active BUY remains untouched;
- creator-vault custom 2006 rows are classified as source-authority mismatch
  or explicitly unverified;
- amount custom 6002 rows expose provided/required/shortfall if logs contain
  parseable values;
- no expected creator-vault value from simulation logs is used as a runtime
  repair source.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

### R15 J3K5 Runtime Result

`r15-bounded-j3k5-r1` was stopped early after reaching the bounded transport
target:

```text
probe_selection_rows = 13
probe_skips_rows = 3
probe_transport_rows = 10
probe_shadow_entry_rows = 9
probe_required_exact_decision_v3_join_coverage = 1.0
active_buys_rows = 0
```

Observed materialization classes:

```text
entry_materialized = 8
transport_only_missing_token_quantity = 1
simulation_slippage_or_price_mismatch:custom_6002 = 1
```

No `custom_2006` row appeared in this run, so creator-vault authority fields are
code/test validated but not runtime-observed here. `custom_6002` was observed,
but this row did not include parseable Anchor `Left/Right` amount values, so the
amount guard status is `amount_guard_values_unavailable`.

Decision: J3K5 is a bounded smoke MINIMAL PASS / DIAGNOSED. Collection remains
HOLD until amount-error diagnostics are sufficient for scaling.

### R15 J3K5 R2 Runtime Result

After the R1 smoke, the amount guard parser was extended to handle inline Anchor
logs:

```text
Program log: Left: <value>
Program log: Right: <value>
```

`r15-bounded-j3k5-r2` was then stopped after the bounded transport target:

```text
probe_selection_rows = 19
probe_skips_rows = 7
probe_transport_rows = 10
probe_shadow_entry_rows = 9
probe_required_exact_decision_v3_join_coverage = 1.0
active_buys_rows = 0
```

Observed materialization classes:

```text
entry_materialized = 7
transport_only_missing_token_quantity = 1
simulation_account_layout_mismatch:custom_2006 = 2
```

Creator-vault source-authority diagnostics were runtime observed:

```text
creator_vault_authority_status_counts = {"creator_vault_source_not_authoritative": 2}
creator_vault_mismatch_reason_counts = {"actual_expected_mismatch": 2}
creator_identity_source_counts = {"account_overrides.creator_pubkey": 2}
```

No `custom_6002` row appeared in R2, so amount guard parsing is code/test
validated but not re-observed at runtime after the inline parser fix.

Decision: J3K5 remains MINIMAL PASS / DIAGNOSED. Collection remains HOLD.
The next repair path is creator-vault source authority / route identity, not
amount sizing.

## P3.7-J3K3 Bounded R1 Early Stop and J3K4 Simulation Error Diagnostics

### Trigger

J3K3 fixed the dominant `missing_bonding_curve` handoff class by allowing the
probe plane to use the decision-time V3/MFS legacy curve snapshot as a
probe-only fallback for legacy buy account overrides. The R15-r10-j3k3 smoke
validated probe transport/entry with exact decision/V3 joins.

A first small bounded run was then started:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k3-r1
```

The run was stopped early instead of waiting for timeout because the initial
artifact snapshot showed simulation errors. This follows the operational rule
for J3 work: do not let a bounded run continue blindly when a new failure class
is visible early.

### Bounded R1 Result

Final bounded-r1 snapshot after early stop:

```text
v3_rows = 5
strict_full_replay = full_replay_ok
probe_selection_rows = 26
probe_transport_rows = 17
probe_shadow_entry_rows = 16
probe_lifecycle_rows = 0
probe_required_exact_decision_v3_join_coverage = 1.0
probe_entry_materialized = 13
simulation_error_rows = 3
transport_only_missing_token_quantity = 1
active_buy_rows = 0
```

The probe transport/entry path remains valid for execution-ready rows, but the
run is not clean enough to scale.

Simulation-error classes:

```text
simulation_account_layout_mismatch:custom_2006 = 2
simulation_slippage_or_price_mismatch:custom_6002 = 1
```

The `custom_2006` rows are Pump.fun Anchor `ConstraintSeeds` failures on
`creator_vault`: the transaction used a creator-vault PDA derived from the
local `creator_pubkey`, while the program log exposed a different expected
creator-vault PDA.

### J3K4 Diagnostic Repair

J3K4 adds additive simulation diagnostics to the probe transport schema:

```text
simulation_error_account_role
simulation_error_account_pubkey
simulation_error_actual_account_pubkey
simulation_error_expected_account_pubkey
```

For Anchor constraint failures, the runtime now parses:

```text
AnchorError caused by account: <role>
Program log: Left:
Program log: <actual_pubkey>
Program log: Right:
Program log: <expected_pubkey>
```

This does not change active decisions, live behavior, IWIM, thresholds, P2, or
probe dispatch semantics. It only turns `custom_2006` from a generic layout
mismatch into an auditable `creator_vault` actual-vs-expected mismatch.

### Next Gate

Do not continue broader collection from bounded-r1 alone. The next decision must
use the bounded-r1 report:

- if `creator_vault` mismatch remains rare and fully classified, allow another
  small bounded run with stop-loss gates;
- if it grows, open a creator-vault materialization/route repair;
- if `custom_6002` grows, open an amount/slippage calibration guard;
- if lifecycle remains absent after clean entry rows, open lifecycle-specific
  validation rather than modifying probe selection.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

### J3K4 Runtime Validation Update

The follow-up diagnostic namespace was executed:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k4-r1
```

Result:

```text
probe_selection_rows = 12
probe_transport_rows = 10
probe_shadow_entry_rows = 10
probe_required_exact_decision_v3_join_coverage = 1.0
entry_materialized = 8
simulation_error_rows = 2
active_buy_rows = 0
```

J3K4 successfully populated structured Anchor constraint diagnostics for
`custom_2006`:

```text
simulation_error_account_role = creator_vault
simulation_error_actual_account_pubkey = 4D8hkwjsgvn5hrQgJULqxuh5hSX3UEUEe2U9nWpTiyTP
simulation_error_expected_account_pubkey = GdZspP3tLaQQ5jrFixZ2xPmWjshMWEX6K9ynkx2BiXLM
```

This proves the `custom_2006` class is a route/account identity mismatch for
`creator_vault`, not an anonymous `AccountNotFound` or join-key failure.

The next decision is intentionally narrow:

```text
P3.7-J3K5 Creator-Vault Source Authority / Amount Guard Decision
```

J3K5 must decide whether to:

- narrow eligibility when creator identity is not route-authoritative;
- add decision-time-safe creator-vault/source materialization;
- keep `custom_2006` as a diagnosed simulation-error class under stop-loss gates;
- add an amount/slippage guard if `custom_6002` grows.

Do not run broad collection until this decision is explicit.

## P3.7-J3J Readiness Coverage Follow-up After Q6-r2

### Trigger

R15 bounded `q6-r2` validated the counterfactual probe transport/entry path for
execution-ready rows:

```text
v3_rows = 90
strict_replay_status = full_replay_ok
probe_selection_rows = 514
probe_transport_rows = 4
probe_shadow_entry_rows = 4
probe_lifecycle_rows = 0
active_buys_rows = 0
probe exact decision/V3 join = 100%
simulation_error_rows = 0
```

The remaining blocker is coverage/yield:

```text
probe_execution_precheck_failed = 396
missing_bonding_curve = 385
missing_execution_route_identity = 10
missing_payer = 1
```

The effective entry yield is therefore too low for bounded collection. Q6-r2
proved that the probe plane works for ready rows, but not that enough rows are
ready to scale collection.

### J3J-A Readiness Latency Audit

J3J-A extends the existing probe execution-account readiness report with an
offline latency audit. For `missing_bonding_curve` rows, the legacy route can
derive the expected bonding curve identity from `pool_id`, then correlate it
against `DIAG_ACCOUNT_UPDATE_RELAY base_mint=... bonding_curve=...` records in
runtime logs.

Required outputs:

```text
expected_account_role
expected_account_pubkey
expected_account_source
first_account_update_ts_ms
first_account_update_after_decision_ts_ms
first_account_update_after_probe_selected_ts_ms
ready_before_decision
ready_before_probe_selected
ready_after_probe_selected_ms
ready_within_500_ms
ready_within_1000_ms
ready_within_1500_ms
ready_within_3000_ms
wait_would_help_within_500_ms
wait_would_help_within_1000_ms
wait_would_help_within_1500_ms
wait_would_help_within_3000_ms
```

### Q6-r2 Result

The Q6-r2 readiness latency report found:

```text
audited_missing_account_rows = 386
missing_bonding_curve_rows = 385
observed_before_decision = 385
observed_after_probe_selected = 0
never_observed_in_run = 1
wait_would_help_within_1500_ms = 0
bounded_wait_recommendation = not_primary_fix_route_or_materialization_gap
recommended_next_stage = account_coverage_or_route_identity_investigation
```

Interpretation:

- a larger wait window is not justified by Q6-r2 evidence;
- for `missing_bonding_curve`, the expected account was already visible in
  diagnostic account updates before the decision/probe selection;
- the blocker is route/materialization/coverage handoff, not short-lived account
  latency;
- collection remains `HOLD` because the current yield is too low.

### Next Gate

Do not run another blind timeout or scale collection from Q6-r2. The next narrow
stage is account coverage / route identity investigation:

```text
P3.7-J3K2 Probe Account Coverage / Route Identity Handoff
```

It must answer why a row can have `DIAG_ACCOUNT_UPDATE_RELAY` evidence for the
legacy bonding curve before decision time while the probe precheck still emits
`missing_bonding_curve`.

Non-goals remain unchanged:

```text
no P2
no live
no active policy changes
no IWIM changes
no threshold tuning
no post-hoc account guessing
no bypassing strict precheck
```

## P3.7-J3K2 Account Coverage / Route Identity Reconciliation

### Trigger

J3J showed that a bounded wait is not the primary fix for Q6-r2
`missing_bonding_curve` rows:

```text
audited_missing_account_rows = 386
missing_bonding_curve_rows = 385
observed_before_decision = 385
observed_after_probe_selected = 0
wait_would_help_within_1500_ms = 0
```

That result means the counterfactual probe should not keep waiting blindly for
account updates that are already present in local diagnostic truth before the
decision. The remaining question is where the handoff breaks between local
account truth, route identity, materialized decision evidence, account
overrides, prepared request construction and RPC simulation readiness.

### J3K2 Report Result

J3K2 adds a read-only reconciliation report:

```text
scripts/v3_p37_probe_account_reconciliation_report.py
```

The report compares each account-readiness skip against:

- the expected account identity inferred from the skip/precheck reason;
- the exact V3 decision row joined by `ab_record_id` and replay hashes;
- V3/MFS route/account materialization hints;
- `DIAG_ACCOUNT_UPDATE_RELAY` local account truth;
- probe transport/prepared-request evidence if the row reached transport.

Q6-r2 reconciliation found:

```text
audited_missing_account_rows = 396
exact_decision_v3_join_rows = 396
classifications = {
  route_mismatch: 10,
  mfs_has_account_but_overrides_missing: 385,
  builder_required_account_not_in_mfs: 1
}
recommended_fix_paths = {
  route_identity_propagation: 10,
  route_override_propagation: 385,
  execution_account_readiness_materialization: 1
}
diag_seen_before_decision_rows = 385
prepared_request_not_built_rows = 396
recommended_next_fix_path = route_override_propagation
```

### Interpretation

The dominant `missing_bonding_curve` class is not a short-lived account-latency
problem and not a post-transport RPC simulation failure. The account is visible
in DIAG before decision, but the probe path stops before request construction
because the legacy bonding-curve identity is not materialized or handed into
the route/account override path used by the counterfactual probe precheck.

The next fix must therefore be narrow route/override propagation for the probe
plane, with active Gatekeeper policy, IWIM, live sender and thresholds left
unchanged.

### Next Gate

Do not run another blind R15 smoke and do not scale collection from J3K2 alone.
The next implementation step is:

```text
P3.7-J3K3 Route / Override Propagation for Legacy Bonding Curve
```

Acceptance for J3K3:

- preserve exact decision/V3 join;
- preserve strict precheck for true missing execution accounts;
- use decision-time-safe local route/account evidence only;
- do not guess missing accounts post-hoc;
- do not mutate active BUY/live/IWIM/threshold behavior;
- repeat a bounded smoke only after the route/override fix is implemented.

## P3.7-J3K3 Route / Override Propagation for Legacy Bonding Curve

### Trigger

J3K2 classified the dominant Q6-r2 blocker as:

```text
mfs_has_account_but_overrides_missing = 385
recommended_next_fix_path = route_override_propagation
prepared_request_not_built_rows = 396
```

The affected rows stopped before `PreparedBuyRequest` construction because the
counterfactual probe precheck did not have a usable `legacy_buy_curve`, even
though decision-time V3/MFS evidence contained curve reserve information and the
account had been observed in local DIAG before the decision.

### Decision

J3K3 adds a probe-only fallback for legacy bonding-curve route overrides:

- materialize a legacy `BondingCurve` snapshot from decision-time
  `v3_materialized_feature_snapshot.account_features.current_reserves`;
- carry that snapshot through the P37 probe candidate and selection record;
- use it only as a fallback for `legacy_buy_curve` in the P37
  counterfactual probe override path when the normal runtime
  `AccountStateCore` lookup is unavailable after cleanup;
- keep active `derive_buy_account_overrides(...)`, Gatekeeper policy, IWIM,
  live sender and threshold behavior unchanged.

This is not a new policy feature and not a post-hoc account guess. The fallback
is limited to the decision-time V3/MFS snapshot already persisted with the
source decision row and is scoped to the counterfactual shadow probe plane.

### Runtime Contract

The probe path remains fail-closed:

- if the V3/MFS reserve snapshot is absent or invalid, the fallback is absent;
- if pool/mint identity does not match the selected probe row, the fallback is
  rejected;
- strict precheck still blocks true missing execution accounts;
- exact `ab_record_id` / `probe_id` / V3 hash continuity remains required by
  audit before any collection decision.

### Validation

J3K3 is a code-level repair. The required validation set is:

```bash
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
python3 -m py_compile \
  scripts/v3_p37_probe_account_reconciliation_report.py \
  scripts/v3_p37_probe_execution_account_readiness_report.py \
  scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest \
  scripts/test_v3_p37_probe_account_reconciliation_report.py \
  scripts/test_v3_p37_probe_execution_account_readiness_report.py \
  scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py \
  -v
rustfmt --edition 2021 --check ghost-launcher/src/oracle_runtime.rs
git diff --check
```

### Next Gate

J3K3 does not claim runtime PASS by itself. The next runtime gate is a fresh
bounded smoke in a clean namespace after this route/override propagation repair.

Collection, Phase B, P2, live sender and runtime threshold tuning remain
blocked until that smoke proves transport/entry yield with exact decision/V3
continuity and no active BUY mutation.

## P3.7-J3Q5 Probe Amount / Slippage Diagnostic

### Trigger

R15-r8n produced five counterfactual probe transport rows and five probe
shadow-entry rows. Four were simulated and one reached the Pump.fun program but
failed with:

```text
InstructionError(3, Custom(6002))
TooMuchSolRequired
Left = 7000000
Right = 11425995
```

The join-key, V3 replay and active BUY mutation gates were clean. The remaining
question is whether this error is an isolated classified probe error or a
systematic amount/slippage/token-param construction problem.

### Finding

All five R15-r8n probes had the same high-level setup:

```text
probe_bucket = v3_reject_manipulation_contradiction
route_kind = legacy_buy
amount_lamports = 7000000
probe_slippage_bps = 2000
```

The error row required `1.632285x` the configured max SOL:

```text
max_sol = 7_000_000 lamports
program_required_sol = 11_425_995 lamports
```

Because R15-r8n predates the new token-param transport fields, Q5 can classify
the family as `simulation_slippage_or_price_mismatch`, but cannot yet
definitively split it into amount-too-large, stale quote, token-param mismatch
or buy-variant mismatch.

### Decision

Do not go directly to the first 25+ probe collection from R15-r8n alone. Run one
tiny token-param-aware smoke first:

```text
P3.7-J3Q5b token-param-aware smoke
```

Acceptance for Q5b:

- probe transport and entry rows exist;
- exact decision/V3 join remains 100%;
- transport rows carry `buy_variant`, `token_param_role`,
  `entry_token_amount_raw`, and `min_tokens_out`;
- `TooMuchSolRequired`, if present, is sub-classified;
- active BUY remains absent;
- no live/P2 path is touched.

If Q5b shows isolated and well-classified amount/slippage errors, a small
bounded collection can start with explicit error-class reporting. If Q5b shows
systematic `TooMuchSolRequired`, repair probe amount/quote construction or run a
tiny amount-variant smoke before collection.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

## P3.7-J3Q4 Probe Simulation Instruction Error Classification

### Trigger

R15-r8m produced the first runtime-validated counterfactual probe transport and
entry rows:

```text
probe_transport_rows = 4
probe_shadow_entries_rows = 4
join_key_audit = PASS
active buys.jsonl = missing
```

One of the four probe rows had a simulation mismatch:

```text
err = InstructionError(3, Custom(2006))
probe_bucket = v3_reject_manipulation_contradiction
```

The R15-r8m transport row predates Q4 diagnostics, so it preserved the error
string but not the simulation logs, failing program id, instruction account
roles or route kind. Scaling collection with that class still unclassified
would risk producing ambiguous probe error rows.

### Decision

J3Q4 adds probe-specific simulation instruction diagnostics:

- parse `InstructionError(<index>, Custom(<code>))`;
- persist `simulation_error_instruction_index`;
- persist `simulation_error_custom_code`;
- persist failing instruction program id/name when the prepared transaction is
  available;
- persist best-effort custom error mapping;
- persist simulation log digest/tail;
- persist failing instruction account pubkeys and probe account roles;
- persist route kind and required account role set;
- add a report script for current and future probe transport rows.

For `Custom(2006)`, the diagnostic mapping is deliberately best-effort unless a
program id/log tail is present. With Pump.fun program id it is classified as:

```text
program_error_name = anchor_constraint_seeds
program_error_family = anchor_account_constraint
simulation_error_category = simulation_account_layout_mismatch
```

Without program/log/account-role fields, current pre-Q4 rows remain:

```text
simulation_account_layout_mismatch_unclassified_missing_q4_fields
```

### Current R15-r8m Classification

The current R15-r8m error is parsed but not fully attributable:

```text
instruction_index = 3
custom_code = 2006
program_id = unknown
category = simulation_account_layout_mismatch_unclassified_missing_q4_fields
```

This is enough to block scaling and require either a small follow-up smoke with
Q4 diagnostics enabled or direct classification from a reproduced simulation
row.

### Gate After Q4

Next runtime evidence must not wait blindly for timeout. Stop as soon as:

- a simulation error appears with Q4 diagnostic fields; or
- probe transport/entry rows appear with zero simulation mismatch; or
- a new structural blocker dominates.

Small bounded collection can proceed only if:

- V3/MFS strict replay remains OK;
- probe transport/entry rows exist;
- join-key audit remains PASS;
- active BUY remains unchanged;
- simulation error class is either absent or classified well enough to avoid
  poisoning labels.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

## P3.7-J3Q3 Optional `bonding_curve_v2` Probe Precheck Repair

### Trigger

R15-r8l was stopped early after the first structural signal. It produced exact
decision/V3 joins, but no probe transport or entry rows:

```text
probe_selection_rows = 20
probe_transport_rows = 0
probe_entry_rows = 0
missing roles = bonding_curve_v2:18, creator_vault:1
```

Manual inspection of one selected pool showed that a real successful on-chain
buy for the same pool used the same `bonding_curve_v2` pubkey as account index
16 of the extended Pump.fun buy instruction, while that account had zero
pre/post lamports and `getAccountInfo` returned no current account data. That
means the probe precheck was treating an optional/zero-lamport remaining
account as a strict account-existence requirement.

### Decision

J3Q3 narrows the counterfactual probe precheck only for this observed optional
role:

- `bonding_curve_v2` is not required to pass RPC existence precheck when it is
  exactly account index 16 of the prepared extended buy instruction;
- the account remains serialized in the prepared instruction and may still be
  used by the simulator;
- `creator_vault`, payer, bonding curve, associated bonding curve and other true
  execution accounts remain strict;
- active BUY, IWIM, live sender, thresholds and V2/V2.5 policy are unchanged.

This is not a collection gate. It is a probe-only correction to avoid rejecting
rows before simulation solely because an observed optional remaining account is
absent on RPC.

### R15-r8m Gate

R15-r8m must prove that the optional `bonding_curve_v2` repair reaches real
counterfactual probe transport and entry rows without mutating active BUY:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8m
```

Minimal pass:

- V3/MFS strict replay remains OK.
- Probe selection exact decision/V3 join remains 100%.
- Probe transport rows are produced.
- Probe shadow entry rows are produced.
- Probe rows carry `dispatch_source = counterfactual_shadow_probe`.
- `buys.jsonl` remains absent or empty.
- Lifecycle close is not required for this gate.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

## P3.7-J3P Probe Legacy Route Preservation Through Preparation

### Trigger

R15-r8i confirmed that the configured shadow payer is no longer the active
blocker. The run used `payer_strategy="configured"` with the historical
shadow-burnin test keypair and preflight accepted the payer account. Probe
selection and decision/V3 hash continuity also remained healthy.

However, selected probes still stopped before transport/entry:

```text
probe_selection_rows = 11
probe_transport_rows = 0
probe_entry_rows = 0
execution_account_not_ready:bonding_curve_v2 = 8
execution_account_not_ready:creator_vault = 2
```

Inspection of the trigger preparation path showed that `LegacyBuy` evidence
could be recovered by the P3.7 probe resolver, but
`prepare_buy_request_with_tip_telemetry_and_amount_lamports` sanitized
`LegacyBuy` back to `None`. The later build step defaulted `None` to
`RoutedExactSolIn`, reintroducing routed-only required accounts
`bonding_curve_v2` and `creator_vault`.

### Decision

J3P keeps the generic active sanitizer conservative, but adds a prepared-request
sanitizer that preserves `LegacyBuy` only when the request carries
`legacy_buy_curve` proof. Without that curve proof, `LegacyBuy` still fails
closed and falls back to the existing behavior.

This is a narrow preparation-boundary repair:

- no active policy change,
- no IWIM change,
- no threshold change,
- no live sender change,
- no global precheck disable,
- no treatment of missing execution accounts as success.

### R15-r8j Gate

The next smoke must use a clean namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8j
```

Acceptance:

- V3/MFS strict replay remains OK.
- Selection exact decision/V3 join remains 100%.
- Configured shadow payer remains in use.
- Legacy probe candidates that carry `legacy_buy_curve` remain `LegacyBuy`
  through prepared-request construction.
- Legacy probe candidates do not require routed-only accounts
  `bonding_curve_v2` / `creator_vault`.
- If no probe transport/entry rows appear, the blocker must be a newly precise
  readiness/simulation class, not loss of `LegacyBuy` at the preparation
  boundary.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

## P3.7-J3N Simulation Payer Account Contract Repair

### Trigger

R15-r8f was stopped early after the first useful blocker appeared. The run
showed that the counterfactual probe plane still did not reach probe
transport/entry rows, while the ordinary shadow transport path produced one
active shadow BUY simulation row:

```text
active_buy_rows = 1
active_shadow_execution_outcome = counterfactual/ordinary shadow simulation error
active_shadow_error = AccountNotFound
active_shadow_payer_provenance = ephemeral
```

The failed prepared-buy log identified the active shadow payer as a
launcher-local ephemeral key. Direct RPC checks showed that this ephemeral fee
payer did not exist on-chain. The configured local rollout wallet from `.env`
and `wallets/shadow-burnin-test.json` resolves to a chain-visible account with
lamports available for simulation.

### Decision

J3N restores the simulation payer contract for the R15 smoke lane:

- use a configured, chain-visible simulation payer for R15-r8h;
- keep `entry_mode = "shadow_only"` and `execution_mode = "shadow"`;
- keep `funding_lane_mode = "disabled"`;
- do not enable live sender, P2, active policy changes, IWIM changes or
  threshold tuning;
- treat the configured payer as simulation infrastructure only, not live
  inclusion;
- add `payer_pubkey` to shadow transport records additively so future
  `AccountNotFound` rows identify the payer directly in JSONL.

### R15-r8h Gate

R15-r8h must use a clean bounded smoke namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8h
```

Smoke profile differences from R15-r8g:

```text
[trigger]
keypair_path = "../../wallets/shadow-burnin-test.json"

[trigger.shadow_run]
payer_strategy = "configured"
```

Acceptance:

- V3/MFS strict replay remains OK.
- Probe selection exact decision/V3 join remains 100%.
- Active shadow `AccountNotFound` must no longer be attributable to a missing
  ephemeral fee payer.
- If `AccountNotFound` remains, `buys.jsonl` must carry `payer_pubkey` and
  `payer_provenance` for immediate diagnosis.
- Probe transport/entry remains the target gate; if no rows appear, the run must
  be stopped early and classified by the first concrete blocker.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

## P3.7-J3M Probe/Shadow Route-Source Compatibility Repair

### Trigger

R15-r8f was stopped early once the blocker was visible:

```text
v3_rows = 15
strict_replay = full_replay_ok
probe_selection_rows = 30
probe_skips = 76
probe_transport_rows = 0
probe_entry_rows = 0
active_shadow_buy_rows = 1
active_shadow_execution_outcome = shadow_data_problem / AccountNotFound
```

J3L correctly moved missing route identity out of the expensive scan path, but
the smoke still showed two route/account compatibility failures:

- counterfactual probe candidates either had no execution route identity or
  failed strict execution-account readiness on `bonding_curve_v2` /
  route-specific `creator_vault`;
- a non-probe active shadow BUY in the same namespace also failed with
  `AccountNotFound`.

This means the remaining blocker is not only probe-plane throttling. The runtime
route/account metadata reaching shadow simulation can be incompatible with the
actual source transaction route.

### Decision

J3M repairs route-source compatibility at the parser/enrichment boundary before
running another smoke:

- do not bypass `bonding_curve_v2`, `creator_vault`, or any strict required
  execution account;
- do not reinterpret `AccountNotFound` as success;
- do not widen probe dispatch limits;
- preserve shadow/live separation and active verdict semantics;
- prefer a true routed pump.fun buy instruction over a legacy-like buy
  instruction when both are present in the same source transaction and both
  match the same trade;
- keep top-level source instructions preferred over inner CPI instructions once
  the top-level match is complete.

The purpose is to stop handing inconsistent route metadata to the trigger
builder. It is not a policy change and it does not enable P2/live.

### Acceptance

- Existing legacy-only and routed-only enrichment tests still pass.
- A transaction containing both top-level legacy-like and routed pump.fun buy
  instructions enriches `buy_variant = routed_exact_sol_in` and carries the
  routed account fields.
- Top-level pump.fun instructions still take priority over inner CPI fallback
  when top-level enrichment is complete.
- R15-r8f is documented as `NOT_READY_DIAGNOSED`; no collection is started.
- Next runtime smoke must again be treated as an early-failure detector.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

## P3.7-J3L Probe Route-Identity Pre-Scan Gate

### Trigger

R15-r8e was stopped early once the structural blocker was clear:

```text
probe_selection_rows = 58
probe_transport_rows = 0
probe_entry_rows = 0
probe_execution_precheck_failed = 25
execution_account_not_ready = 31
missing_execution_route_identity = 25
execution_account_not_ready:bonding_curve_v2 = 28
execution_account_not_ready:creator_vault = 3
```

J3K correctly made missing route identity fail closed, but the check still ran
inside the background scan path. That meant rows with no usable execution route
identity consumed scan-plane capacity before being classified.

### Decision

J3L moves the cheap route-identity precheck before scan admission:

- derive account overrides from the current buffered transaction evidence and
  pool metadata before `try_reserve_scan_slot`;
- if route identity is missing, write a structured `probe_skipped` row
  immediately;
- do not reserve scan budget, spawn the background probe task, wait for account
  readiness, or touch dispatch quota for route-unready rows;
- keep the same fail-closed route reasons:
  `missing_execution_route_identity`,
  `missing_routed_associated_bonding_curve`, and `missing_creator_pubkey`;
- preserve strict execution-account readiness after route identity passes.

This is not a policy change and does not loosen any execution account precheck.
It only prevents route-unready rows from occupying the scan plane.

The readiness audit is also updated to include pre-scan precheck skips that do
not have a paired `probe_selected` row. Those rows are classified explicitly as
route-identity failures instead of disappearing from the readiness report or
being folded into `unknown`.

### R15 Post-J3L Gate

The next smoke must use a fresh namespace and must again be treated as an
early-failure detector:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8f
```

Acceptance:

- V3/MFS strict replay remains OK.
- Probe selection exact decision/V3 join remains 100%.
- Route-unready rows are visible in `probe_skips.jsonl` but do not consume scan
  budget.
- If complete route identity exists but strict execution accounts are missing,
  the blocker remains `execution_account_not_ready` with role/pubkey.
- If probe transport/entry appears, stop after a short grace period and produce
  reports immediately.
- If route/precheck skips dominate and no transport/entry appears, stop early
  and repair the next structural blocker; do not wait for timeout.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

## P3.7-J3K Execution Route Identity Gate

### Trigger

R15-r8d after J3J proved that a short bounded wait does not make the currently
selected probe rows execution-ready:

```text
probe_selection_rows = 25
probe_transport_rows = 0
probe_entry_rows = 0
wait_timeout = 21
missing_roles = bonding_curve_v2:19, creator_vault:2
```

At that point the blocker is no longer scan throughput, dispatch quota, payer
semantics, or short-lived account readiness. The remaining risk is that the
probe build path can construct a routed shadow simulation request from
incomplete decision-time execution route identity, then fail later on derived
route accounts such as `bonding_curve_v2` or `creator_vault`.

### Decision

J3K makes execution route identity an explicit fail-closed eligibility
condition before the probe request is built:

- `buy_variant` must be present in the derived account overrides;
- routed exact-SOL-in probes must carry `associated_bonding_curve`;
- `creator_pubkey` must be present before creator-vault-dependent routed
  requests are allowed;
- missing route identity is logged as a structured precheck skip, not as a
  late `AccountNotFound` or `execution_account_not_ready` after request build.

This is intentionally conservative. It does not infer route accounts
post-factum and does not treat missing route identity as success. It converts a
late simulation/build blocker into a decision-time-safe eligibility result.

New skip reasons:

```text
missing_execution_route_identity
missing_routed_associated_bonding_curve
missing_creator_pubkey
```

Strict execution accounts remain strict. `bonding_curve_v2` and
route-specific `creator_vault` are still not bypassed if the route identity is
complete and the account is actually missing.

### R15 Post-J3K Gate

The next runtime smoke must use a fresh namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8e
```

It must be treated as an early-failure detector:

- stop early if route-identity skips dominate and no transport/entry rows
  appear;
- stop early if probe transport/entry rows appear;
- generate reports immediately at the stopping point.

Acceptance:

- V3/MFS strict replay remains OK.
- Probe selection exact decision/V3 join remains 100%.
- Route identity failures become explicit precheck skips.
- No probe collection is allowed while probe transport/entry remain absent.
- If complete route identity is present but strict execution accounts are still
  missing, the blocker remains execution-account readiness and must be reported
  precisely.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

## P3.7-J3J Execution Account Wait Strategy

### Trigger

R15-r8c after J3I3 proved that scan-plane concurrency was no longer the active
blocker:

```text
probe_selection_rows = 33
probe_scan_concurrency_limit_exceeded = 0
probe_transport_rows = 0
probe_entry_rows = 0
dominant blocker = execution_account_not_ready
```

The dominant missing roles remained strict execution accounts, primarily
`bonding_curve_v2`. That means the probe plane is reaching decision rows with
valid V3/MFS metadata and exact join keys, but the execution account set is not
ready at immediate probe-dispatch time.

### Decision

J3J adds a bounded, decision-time-safe wait for required execution accounts in
the isolated counterfactual probe background path:

- new config field: `probe_wait_for_execution_accounts_ms`;
- default is `0`, preserving fail-fast behavior for legacy and non-probe
  configs;
- the wait runs only after probe selection and scan admission, and before
  dispatch quota is consumed;
- the decision hot path remains non-blocking;
- active Gatekeeper verdicts, IWIM, live sender and thresholds are unchanged;
- strict execution accounts remain strict and are not bypassed;
- wait diagnostics are written on probe selection/skip/transport rows as
  `probe_execution_account_wait_ms` and
  `probe_execution_account_wait_result`.

Valid wait results:

```text
ready_without_wait
wait_disabled
ready_after_wait
wait_timeout
```

Rows that still lack a strict required account after the wait remain
`execution_account_not_ready:<role>:<pubkey>`. This is a diagnostic narrowing
step, not a success condition.

### R15 Post-J3J Gate

The next smoke must use a fresh namespace because previous `append=false`
namespaces already contain probe artifacts:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8d
```

Smoke must be monitored as an early-failure detector. Stop early once one of
the following is clear:

- probe transport/entry rows appear;
- `wait_timeout` plus `execution_account_not_ready` dominates;
- a new structural blocker appears.

Acceptance:

- V3/MFS strict replay remains OK.
- Probe selection exact decision/V3 join remains 100%.
- If account readiness arrives within the wait window, probe transport/entry
  can be produced.
- If no account readiness arrives, the report must classify the run as
  `NOT_READY_DIAGNOSED` with wait diagnostics, not as timeout noise.
- Full/bounded collection remains `HOLD` until probe transport/entry pass.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

## P3.7-J3I3 Probe Scan Backlog Admission Repair

### Trigger

R15-r8 and the immediately aborted R15-r8b smoke attempt showed that the J3I2
contract still had one practical flaw: `probe_scan_concurrency_limit_exceeded`
could dominate early even when `max_probe_candidates_scanned_per_run` had not
been reached.

That means scan concurrency was acting as candidate admission control instead
of as a bound on active readiness work. In that mode a useful probe candidate
could be discarded simply because another readiness check was in flight.

### Decision

J3I3 changes scan-plane semantics:

- candidate admission is bounded by `max_probe_candidates_scanned_per_run` and
  `dedupe_by_probe_id`;
- scan concurrency is enforced by awaiting a scan semaphore inside the
  background probe task;
- the decision hot path remains non-blocking;
- `probe_scan_concurrency_limit_exceeded` is no longer a normal skip reason;
- dispatch quota is still consumed only after execution-account readiness has
  passed;
- the finite scan backlog is bounded by the configured candidate scan limit.

If the scan semaphore is closed, the probe is written as
`probe_scan_semaphore_closed`. That is an internal runtime shutdown/error
condition, not a data-readiness conclusion.

This repair does not relax `bonding_curve_v2`, `creator_vault`, payer, or any
other execution-account precheck. It only prevents premature candidate loss
before the strict readiness check has run.

### R15 Post-J3I3 Gate

The next runtime smoke must be treated as an early-failure detector, not as a
blind timeout wait:

- stop immediately if a new structural blocker dominates early;
- generate the current reports at the stopping point;
- repair the blocker before running a larger collection;
- do not proceed to collection while probe transport/entry remain absent.

Acceptance:

- V3/MFS strict replay remains OK.
- Probe selection exact decision/V3 join remains 100%.
- `probe_scan_concurrency_limit_exceeded` is absent or replaced by bounded
  waiting.
- If no probe transport/entry rows appear, the dominant blocker must be a real
  readiness class such as `execution_account_not_ready`, not scan-plane
  throttling.
- Full/bounded collection remains `HOLD` until probe transport/entry pass.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

## P3.7-J3I2 Probe Scan-Plane Throughput Repair

### Trigger

R15-r7 was manually stopped before natural timeout after producing a useful
final diagnostic snapshot:

```text
v3_rows = 199
strict_replay = full_replay_ok
probe_selection_rows = 548
probe_transport_rows = 0
probe_entry_rows = 0
execution_account_not_ready = 543
probe_scan_concurrency_limit_exceeded = 283
```

J3I successfully separated not-ready rows from dispatch quota, but the scan
plane itself remained too narrow. `probe_scan_concurrency_limit_exceeded` means
the run did not prove absence of execution-ready rows in the candidate universe;
it only proved that rows reaching strict readiness were not ready.

### Decision

J3I2 separates scan throughput from dispatch concurrency:

- `max_scan_concurrent` controls candidate readiness scans;
- `max_concurrent` remains the dispatch-only concurrency limit;
- `max_probe_candidates_scanned_per_run` provides an optional finite scan budget
  for smoke runs;
- exceeding scan concurrency or scan-count budget logs a skip and does not
  consume dispatch quota;
- dispatch quota is still consumed only after
  `execution_account_readiness_status = ready`.

This is not a precheck bypass and not a collection gate. `bonding_curve_v2`
remains strict, `creator_vault` remains route-aware, and missing execution
accounts remain `execution_account_not_ready`.

### R15-r8 Gate

R15-r8 must use a clean bounded smoke namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8
```

Smoke profile:

```text
max_probes_per_run = 5
max_probes_per_minute = 5
max_concurrent = 1
max_scan_concurrent = 8
max_probe_candidates_scanned_per_run = 1000
```

Acceptance:

- V3/MFS strict replay remains OK.
- Probe selection exact decision/V3 join remains 100%.
- `probe_scan_concurrency_limit_exceeded` is no longer the dominant reason that
  prevents readiness evaluation.
- Not-ready rows still do not consume dispatch quota.
- If execution-ready rows exist, dispatch can reach probe transport/entry.
- If no execution-ready rows exist after the finite scan budget, the run is
  `NOT_READY_DIAGNOSED`, not a scan-throughput artifact.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

## P3.7-J3O Probe Variant-Aware Buy Route / Required Account Contract

### Trigger

R15-r8h validated the configured shadow simulation payer path, but the next
early blocker was no longer the wallet. The run used the historical configured
shadow-burnin payer and still stopped before probe transport/entry because
strict execution-account readiness saw missing builder-derived accounts:

```text
execution_account_not_ready:bonding_curve_v2:<pubkey>
execution_account_not_ready:creator_vault:<pubkey>
missing_execution_route_identity
```

Manual inspection of the same selected pool showed observed `legacy_buy` and
`routed_exact_sol_in` events in the buffered transaction set. The direct
builder, however, treated legacy and routed buys as if they shared the newer
extended account layout. That made probe precheck demand routed-only execution
accounts for rows that could be simulated with the compact legacy buy layout.

### Decision

J3O separates the probe account contract by buy variant:

- `LegacyBuy` uses the compact Pump.fun account layout observed on-chain:
  global, fee recipient, mint, bonding curve, associated bonding curve,
  user ATA, payer, system program, token program, rent, event authority and
  Pump program.
- `RoutedExactSolIn` keeps the extended route with creator vault, volume
  accumulator, fee config/program, bonding curve v2 and buyback fee recipient.
- `creator_pubkey` remains route metadata used to derive route-specific accounts;
  it is not itself a transaction account and must not be required as an RPC
  account.
- The P3.7 probe plane may preserve a trusted observed `legacy_buy` route from
  buffered transaction evidence before falling back to the generic active
  override resolver.
- The active generic override resolver still drops unverified legacy route
  overrides; J3O is a probe-only compatibility path for counterfactual shadow
  simulation evidence.

### R15-r8i Gate

The next smoke must use a clean namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8i
```

Acceptance:

- V3/MFS strict replay remains OK.
- Selection exact decision/V3 join remains 100%.
- Configured shadow payer remains in use.
- Legacy probe candidates no longer require `bonding_curve_v2` or
  `creator_vault`.
- Routed probe candidates still require route-specific execution accounts.
- If no probe transport/entry rows appear, the blocker must be a newly precise
  readiness class, not the legacy/routed account-layout mismatch.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.

## P3.7-J3Q4 Simulation Instruction Error Classification

### Trigger

R15-r8m was the first smoke that produced useful counterfactual probe
transport and entry rows. One probe entry surfaced a raw simulation mismatch:

```text
InstructionError(3, Custom(2006))
```

At that point the transport record did not preserve enough simulation context
to classify the error. The row lacked program id, instruction account roles and
simulation log tail, so the result could only be treated as
diagnostic-limited.

### Decision

J3Q4 extends probe simulation diagnostics without changing active decision
behavior:

- propagate simulation instruction index and custom error code;
- propagate simulation program id and best-effort program name;
- propagate instruction account pubkeys and probe account-role labels;
- propagate route kind and required account roles;
- preserve a bounded simulation log tail;
- classify known Pump.fun/Anchor custom errors as explicit diagnostic classes.

Rows that predate these fields remain parsable but are explicitly
`diagnostic-limited`.

### R15-r8n Result

R15-r8n used a clean namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8n
```

Observed result:

```text
probe_selection_rows = 17
probe_transport_rows = 5
probe_shadow_entry_rows = 5
probe_lifecycle_rows = 0
probe_join_key_acceptance = pass
probe_decision_join_acceptance = pass
probe_required_exact_decision_v3_join_coverage = 1.0
active_buys_jsonl = missing
```

The counterfactual probe transport/entry path is therefore runtime-validated
for a bounded smoke namespace. Lifecycle/on-chain labels are not validated yet
because no probe lifecycle close was observed.

One transport/entry row produced:

```text
InstructionError(3, Custom(6002))
program = pumpfun
program_error_name = too_much_sol_required
category = simulation_slippage_or_price_mismatch
Left = 7000000
Right = 11425995
```

This is not an `AccountNotFound` class and not a join-key/hash-continuity class.
It is an amount/slippage mismatch in the probe buy request.

### Next Gate

Do not start a larger collection from J3Q4 alone. The next narrow step is:

```text
P3.7-J3Q5 Probe Amount / Slippage Semantics
```

J3Q5 must decide whether `TooMuchSolRequired` should remain a classified probe
error row, whether the probe request amount/quote construction is wrong, or
whether the smoke profile amount/slippage needs a probe-only adjustment.

To make that decision auditable, future probe transport rows now carry the
request-side buy parameter context:

```text
buy_variant
token_param_role
entry_token_amount_raw
min_tokens_out
amount_lamports
probe_amount_source
probe_slippage_bps
```

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.
