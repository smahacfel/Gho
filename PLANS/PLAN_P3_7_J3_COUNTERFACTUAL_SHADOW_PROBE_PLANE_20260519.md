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
