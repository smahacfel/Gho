# Shadow Burnin V2 Simulation Contract 2026-06-29

## Status

```text
PR12_PR13_READY_FOR_REVIEW
```

Ten dokument definiuje kontrakt Shadow Burnin Simulation V2. Nie aktywuje runtime, nie zmienia BUY/REJECT, nie zmienia Gatekeeper policy, nie zmienia selector runtime, nie zmienia TX/Jito/live path, nie włącza `shadow_close_only` i nie włącza active close.

## Baseline

Baseline fidelity verdict:

```text
SHADOW_REPLAY_LIFECYCLE_MISMATCH
```

Konsekwencje:

- Shadow V1 nie jest live-equivalent.
- Shadow V1 nie może być unified position truth.
- `shadow_exit_replay_v1` jest offline path/label evidence.
- `shadow_lifecycle` nie jest wystarczającą prawdą terminalną dla replay.
- Stare raporty nie mogą być proof of live PnL, executable fills, live slippage behavior ani real landing outcome.

## Measurement Layers

### `MARK_PRICE_REPLAY`

Offline mark/path evidence.

Może wspierać:

- target/stop/timeout labels,
- path-derived MFE/MAE,
- density/horizon reports,
- offline relative research under explicit assumptions.

Nie może wspierać:

- executable fill claim,
- live fill claim,
- live slippage claim,
- landing claim,
- runtime approval.

### `EXECUTABLE_FILL_SIM`

Causal fill simulation.

Wymaga:

- pool-state provenance,
- event ordering,
- clock-domain declarations,
- latency model,
- slippage model,
- own-impact model,
- fee model,
- failed-landing/no-fill model,
- quote/fill divergence model or measurement.

### `LIVE_CONFIRMED`

Actual landed and reconciled transaction evidence.

Jest calibration data, a nie shadow assumption. Bez live-confirmed calibration dataset nie wolno użyć verdict:

```text
SHADOW_V2_LIVE_EQUIVALENCE_GRADE
```

## Common Envelope

Każdy rekord V2 musi zawierać:

- `schema`
- `schema_version`
- `simulation_contract_version`
- `simulation_level`
- `measurement_grade`
- `run_id`
- `session_id`
- `candidate_id`
- `position_id`
- `event_id`
- `parent_event_id`
- `source_event_id`
- `pool_id`
- `base_mint`
- `bonding_curve`
- `produced_at_ms`
- `produced_at_slot`
- `temporal_class`
- `clock_domain`
- `source_refs`
- `quality`
- `limitations`

`simulation_level` enum:

- `MARK_ONLY`
- `FILL_MODEL_STATIC`
- `FILL_MODEL_CALIBRATED`
- `LIVE_CONFIRMED`

`measurement_grade` enum:

- `DIAGNOSTIC_ONLY`
- `MARK_PRICE_REPLAY`
- `RESEARCH_GRADE_CANDIDATE`
- `SHADOW_V2_RESEARCH_GRADE`
- `SHADOW_V2_RESEARCH_GRADE_ONLY`
- `SHADOW_V2_LIVE_EQUIVALENCE_CANDIDATE`
- `SHADOW_V2_LIVE_EQUIVALENCE_GRADE`
- `BLOCKED_BY_DATA`
- `UNKNOWN`

## Canonical Records

Minimum required records:

- `shadow_position_v2`
- `pool_state_sample_v2`
- `shadow_entry_decision_v2`
- `shadow_entry_attempt_v2`
- `shadow_entry_fill_v2`
- `shadow_path_sample_v2`
- `shadow_exit_attempt_v2`
- `shadow_exit_fill_v2`
- `shadow_terminal_truth_v2`
- `shadow_replay_v2`
- `shadow_lifecycle_v2`

PR1-level Rust contract types include `ShadowPositionV2` and the schema vocabulary for all records above. PR2 owns durable canonical writer/indexing invariants for `shadow_position_event_v2.jsonl`, including duplicate event and duplicate terminal rejection.

The canonical event stream is:

```text
shadow_position_event_v2.jsonl
```

Derived outputs must point back to canonical `event_id` and must not become competing truths.

PR8/PR9-level Rust contract types add pure derived projections for:

- `shadow_replay_v2`
- `shadow_lifecycle_v2`

These projections are generated from `shadow_position_event_v2` in memory in
the current PR branch. They do not activate a runtime writer.

## Event Order Key

Required on:

- `pool_state_sample_v2`
- `shadow_path_sample_v2`
- `shadow_entry_attempt_v2`
- `shadow_entry_fill_v2`
- `shadow_exit_attempt_v2`
- `shadow_exit_fill_v2`

Fields:

- `slot: EventOrderComponent<u64>`
- `block_time: EventOrderComponent<i64>`
- `signature: EventOrderComponent<string>`
- `transaction_index_or_unknown: EventOrderComponent<u32>`
- `instruction_index_or_unknown: EventOrderComponent<u32>`
- `inner_instruction_index_or_unknown: EventOrderComponent<u32>`
- `log_index_or_unknown: EventOrderComponent<u32>`
- `event_seq_in_process`
- `observed_at_wall_ms`

`EventOrderComponent<T>` values are:

- `KNOWN(T)`
- `UNKNOWN`

Rules:

- Missing JSON chain-order field is schema-invalid.
- Unknown chain-order component must be explicit `UNKNOWN`.
- `slot=UNKNOWN` blocks research-ready pool-state provenance.
- `event_seq_in_process` must be monotonic per process/run.
- Same-slot incomplete ordering must produce ambiguity metadata.
- Ambiguity cannot silently resolve target/stop or win/loss.

## Clock-Domain Contract

Every timestamp field must declare a clock domain.

Clock domains:

- `wall_clock_ms`
- `monotonic_process_ms`
- `chain_slot`
- `block_time`
- `stream_observed_ms`
- `rpc_observed_ms`
- `decision_ts_ms`
- `submit_ts_ms`
- `landing_ts_ms`

Rules:

- Clock domains cannot be compared without an explicit conversion rule.
- `decision_ts_ms`, `submit_ts_ms`, and `landing_ts_ms` are event semantics.
- `stream_observed_ms` and `rpc_observed_ms` must remain distinct.
- `block_time` is not a replacement for local observation time.
- Timestamp fields without a domain block research-grade use.

Failure verdict:

```text
BLOCKED_TIMESTAMP_CLOCK_DOMAIN_UNKNOWN
```

## Temporal Classes

Allowed temporal classes:

- `PRE_DETECTION`
- `PRE_DECISION`
- `AT_DECISION`
- `POST_ENTRY`
- `POST_EXIT`
- `OUTCOME`
- `UNKNOWN`

Selection features may use only:

- `PRE_DECISION`
- `AT_DECISION`

Any strategy feature classified as `POST_ENTRY`, `POST_EXIT`, `OUTCOME`, or `UNKNOWN` triggers:

```text
SHADOW_TEMPORAL_LEAKAGE_RISK
```

## Entry Contract

Entry V2 must separate:

- `decision_mark_price`
- `entry_quote_price`
- `entry_fill_price`

PR5-level schema fields:

- `shadow_entry_attempt_v2.decision_mark_price`
- `shadow_entry_attempt_v2.entry_quote_price`
- `shadow_entry_attempt_v2.entry_quote_tokens_out`
- `shadow_entry_attempt_v2.entry_quote_min_out`
- `shadow_entry_fill_v2.fill_price`
- `shadow_entry_fill_v2.fill_price_source`
- `shadow_entry_fill_v2.fill_amount_sol`
- `shadow_entry_fill_v2.fill_amount_tokens`
- `shadow_entry_fill_v2.slippage_bps`
- `shadow_entry_fill_v2.own_impact_bps`
- `shadow_entry_fill_v2.fee_bps`
- `shadow_entry_fill_v2.min_out`

Entry fill status:

- `FILLED`
- `NO_FILL`
- `FAILED`
- `BLOCKED_BY_DATA`

Entry fill is not live-equivalent unless it includes:

- state at causal boundary,
- own buy impact,
- entry slippage,
- fees,
- min-out,
- latency/landing model,
- failure/no-fill model.

### PR4 Deterministic Price Reconstruction Contract

PR4 introduces an inert formula library:

```text
ghost-core/src/shadow_v2_price.rs
```

Formula version:

```text
shadow_v2_constant_product_price_v1
```

The library reconstructs:

- normalized mark price from reserves, token decimals and lamports-per-SOL;
- constant-product BUY quote;
- constant-product SELL quote;
- fee amount in bps;
- configured slippage tolerance and `min_out`;
- own impact bps separated from fee bps;
- post-trade deterministic reserve state.

Rounding contract:

- BUY output must be computed as
  `floor(token_reserves_raw * effective_sol_in / (sol_reserves_lamports + effective_sol_in))`;
- SELL output must be computed as
  `floor(sol_reserves_lamports * token_in_raw / (token_reserves_raw + token_in_raw))`;
- the implementation must not compute output as
  `reserve_before - floor(k / post_reserve)`, because that can overstate output
  by one raw unit;
- off-by-one rounding fixtures are required for BUY and SELL.

The formula library must reject:

- zero SOL reserves;
- zero token reserves;
- zero input amount;
- invalid fee bps;
- invalid slippage bps;
- missing SOL lamports normalization;
- unsupported token decimals;
- zero-output quotes.

PR4 is not runtime execution. It does not read live state, submit transactions,
change BUY/REJECT, change Gatekeeper policy, change selector runtime, change
TX/Jito path, enable `shadow_close_only`, or enable active close.

### PR5 Static Entry Fill Contract

PR5 introduces an inert static entry fill model in:

```text
ghost-brain/src/guardian/post_buy/shadow_v2.rs
```

Model version:

```text
shadow_v2_entry_fill_static_constant_product_v1
```

The model may emit `FILLED` only when:

- the referenced `pool_state_sample_v2` is research-ready;
- the pool-state temporal class is allowed for the entry causal boundary;
- `pool_state_sample_v2.event_order_key` is strictly before the entry fill
  event boundary;
- future pool state, equal process sequence, unknown fill slot, missing fill
  wall-clock observation, or incomplete same-slot order emits
  `BLOCKED_BY_DATA`;
- reserve provenance exists for the selected pool phase;
- token decimals and lamports normalization are explicit;
- input SOL lamports are non-zero;
- fee and slippage bps are valid;
- deterministic quote reconstruction succeeds.

Otherwise it must emit:

```text
fill_status = BLOCKED_BY_DATA
reconstruction_status = ENTRY_FILL_BLOCKED_BY_DATA
```

with explicit blockers in `limitations`.

PR5 static entry fill is:

- `simulation_level = FILL_MODEL_STATIC`;
- `measurement_grade = RESEARCH_GRADE_CANDIDATE` only for filled static model records;
- `measurement_grade = BLOCKED_BY_DATA` for blocked records;
- not `LIVE_CONFIRMED`;
- not live-equivalent without PR14 live-confirmed calibration.

PR5 intentionally records limitations such as:

- no live landing confirmation;
- no failed transaction/no-fill telemetry;
- slippage is configured tolerance, not realized live slippage;
- pool state after fill is deterministic derived state, not observed account state.

## Exit Contract

Exit V2 must separate:

- exact-level hit,
- sampled-path hit,
- mark exit,
- executable sell fill.

Exit fill is not live-equivalent unless it includes:

- state at causal boundary,
- own sell impact,
- exit slippage,
- fees,
- min-out,
- failure/no-fill model.

Same-slot target/stop ambiguity must be explicitly represented.

### PR6 Static Exit Fill Contract

PR6 introduces an inert static exit fill model in:

```text
ghost-brain/src/guardian/post_buy/shadow_v2.rs
```

Model version:

```text
shadow_v2_exit_fill_static_constant_product_v1
```

The model may emit `FILLED` only when:

- the referenced `pool_state_sample_v2` is research-ready;
- the pool-state temporal class is allowed for the exit causal boundary;
- `pool_state_sample_v2.event_order_key` is strictly before the exit fill
  event boundary;
- future pool state, equal process sequence, unknown fill slot, missing fill
  wall-clock observation, or incomplete same-slot order emits
  `BLOCKED_BY_DATA`;
- reserve provenance exists for the selected pool phase;
- token decimals and lamports normalization are explicit;
- input token raw amount is non-zero;
- fee and slippage bps are valid;
- deterministic SELL quote reconstruction succeeds.

Otherwise it must emit:

```text
fill_status = BLOCKED_BY_DATA
reconstruction_status = EXIT_FILL_BLOCKED_BY_DATA
```

with explicit blockers in `limitations`.

PR6 static exit fill is:

- `simulation_level = FILL_MODEL_STATIC`;
- `measurement_grade = RESEARCH_GRADE_CANDIDATE` only for filled static model records;
- `measurement_grade = BLOCKED_BY_DATA` for blocked records;
- not `LIVE_CONFIRMED`;
- not live-equivalent without PR14 live-confirmed calibration;
- not active close;
- not `shadow_close_only`;
- not an executable sell transaction.

PR6 can also emit explicit modeled failure records:

```text
fill_status = NO_FILL
fill_status = FAILED
```

These records are useful as typed simulation outcomes only. They do not prove
failed live landing, no-fill, Jito behavior or sell execution without
live-confirmed telemetry.

PR6 intentionally records limitations such as:

- no live exit transaction confirmation;
- no failed transaction/no-fill telemetry;
- slippage is configured tolerance, not realized live slippage;
- pool state after fill is deterministic derived state, not observed account state;
- static exit fill does not enable active close.

### PR6 Exit Path Replay Contract

PR6 also defines an inert path replay helper:

```text
replay_exit_from_path_v2
```

The helper is a deterministic mark/path replay contract. It is not an
executable sell fill and it must not be treated as active close evidence.

`shadow_exit_path_replay_v2` separates:

- `exact_level_hit`: first target/stop evidence from `LEVEL_HIT` samples;
- `sampled_path_hit`: first target/stop evidence from sampled path points;
- `timeout_path_point`: actual path point used for timeout evidence, or an
  explicit stale/blocked limitation;
- `selected_exit`: selected mark/path exit after target/stop/timeout logic;
- `mfe_mark_bps`, `mae_mark_bps`, `terminal_pnl_mark_bps`.

Target and stop detection rules:

- target and stop are evaluated only on samples with `age_ms <= max_hold_ms`;
- exact-level and sampled-path evidence are recorded independently, even when
  a sampled hit appears before a later exact-level marker;
- exact-level evidence does not become executable sell fill evidence;
- if target and stop have incomplete same-slot order and policy is
  `BLOCK_AMBIGUOUS`, `selected_exit` becomes `BLOCKED_BY_DATA`;
- if a tie-break policy is explicitly configured, the selected path result must
  carry `SAME_SLOT_TIE_BREAK_*` limitations;
- timeout uses a real path point at or before `max_hold_ms`; if only an older
  last-known point exists, it is labeled as stale approximation;
- if no path point exists before timeout, timeout is blocked by data.

## Path Density Contract

Path V2 must separate:

- mark price path samples,
- executable exit quote attachment,
- path sampling mode,
- sampling reason,
- horizon coverage verdict.

PR7 introduces three sampling modes:

```text
shadow_path_dense_3s
shadow_path_standard_120s
shadow_path_long_500s
```

Mode intent:

- `shadow_path_dense_3s`: high-density short horizon for 2s/3s research;
- `shadow_path_standard_120s`: standard path evidence up to 120s;
- `shadow_path_long_500s`: long horizon evidence up to 500s, requiring an
  explicit storage budget before use in a validation burnin.

Sampling reasons are typed:

```text
EVENT_SAMPLE
HEARTBEAT
LEVEL_HIT
LARGE_PRICE_DELTA
TERMINAL
```

`LEVEL_HIT` and `TERMINAL` are must-keep samples.

`shadow_path_dense_3s` additionally keeps every `EVENT_SAMPLE`. This is the
only PR7 mode intended for 2s/3s fidelity research. It requires an explicit
storage budget before a validation burnin.

`max_path_points` is a storage cap for optional samples. It must not drop
protected samples:

- `LEVEL_HIT`;
- `TERMINAL`;
- `EVENT_SAMPLE` when `keep_every_event_sample = true`.

If protected dense samples exceed `max_path_points`, the sampler must retain
them and emit a storage-budget limitation instead of silently truncating the
path. Optional samples may be truncated only with an explicit truncation flag
and limitation.

`shadow_path_long_500s` also requires an explicit storage budget before a
validation burnin. It is the only PR7 mode intended to make 300s/500s horizons
evaluable, and only when actual coverage exists.

Horizon verdicts are:

```text
EVALUABLE_EXACT
EVALUABLE_APPROX
SPARSE_APPROX_ONLY
NOT_EVALUABLE_NO_COVERAGE
NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY
```

Rules:

- Unsupported horizons must be reported as `NOT_EVALUABLE_*`, never inferred.
- 2s/3s research requires dense-mode coverage or an explicit approximation
  label.
- 300s/500s research requires long-mode horizon coverage and generated density
  evidence.
- Same-slot incomplete ordering can remain ambiguous evidence only. It must not
  resolve target/stop or win/loss without an explicit tie-break policy.
- Mark path samples use `MARK_PRICE_REPLAY` until a static executable quote is
  attached.
- Static executable exit quote remains `FILL_MODEL_STATIC`, not live fill.
- Density evaluation must emit duplicate-age and non-monotonic input metadata.
  These labels do not rewrite the path; they make ordering defects visible to
  downstream fidelity gates.
- Sampler output may be truncated only for non-protected samples, and only with
  an explicit truncation flag and limitation.

## Terminal Truth Contract

Each `position_id` may have exactly one canonical terminal event.

Duplicate terminal lifecycle rows are allowed only as typed sub-events or derived views. Silent duplicate terminal truth is invalid.

## PR8 Replay V2 Derived View Contract

PR8 defines `shadow_replay_v2` as a derived view from the canonical event
stream, not a second position truth.

Required PR8 invariants:

- replay is derived from `shadow_position_event_v2`;
- replay carries `canonical_event_stream_ref`;
- replay carries the canonical terminal event id when terminal truth exists;
- replay carries source canonical event ids;
- replay separates mark path samples from static executable quote/fill lane;
- replay emits mark path counts and static executable lane counts;
- replay limitations must include `REPLAY_V2_DERIVED_VIEW_NOT_CANONICAL_TRUTH`;
- replay limitations must include `MARK_REPLAY_NOT_EXECUTABLE_FILL`;
- static executable lane remains `FILL_MODEL_STATIC`, not `LIVE_CONFIRMED`;
- missing canonical position events are an error, not an empty success.

PR8 does not write V1 `shadow_exit_replay_v1`, does not change the old replay
writer and does not make mark/path evidence live-equivalent.

## PR9 Lifecycle V2 Derived View Contract

PR9 defines `shadow_lifecycle_v2` as a lifecycle projection from the same
canonical event stream.

Required PR9 invariants:

- lifecycle is derived from `shadow_position_event_v2`;
- lifecycle carries `canonical_event_stream_ref`;
- lifecycle carries source canonical event ids;
- lifecycle references the canonical position event and canonical terminal
  event when present;
- lifecycle event type is a typed sub-event such as `POSITION_OPEN`,
  `POSITION_CLOSED` or `TERMINAL_BLOCKED`;
- lifecycle sub-events are not canonical terminal truth;
- appending a lifecycle sub-event must not consume the one-terminal invariant;
- lifecycle limitations must include
  `LIFECYCLE_V2_DERIVED_VIEW_NOT_CANONICAL_TERMINAL_TRUTH`;
- lifecycle limitations must include
  `LIFECYCLE_V2_DOES_NOT_IMPLY_LIVE_POSITION_STATE`;
- replay/lifecycle reconciliation must use exact join key only:
  `run_id`, `session_id`, `position_id`, `pool_id`, `base_mint`;
- fallback joins are not accepted;
- exact join mismatch is reported as
  `REPLAY_LIFECYCLE_EXACT_JOIN_KEY_MISMATCH`, not silently repaired.

PR9 does not activate shadow close, active close, live sell, runtime lifecycle
behavior or any strategy proof.

## PR10 Evidence Manifest and Retention Contract

PR10 defines an offline evidence manifest contract for future Shadow V2 validation
burnins. It does not collect evidence, start a run, stop a run, clean artifacts
or stage raw logs.

Required PR10 manifest records:

- `shadow_v2_evidence_manifest_v1`
- `shadow_v2_artifact_manifest_entry_v1`

Required PR10 manifest fields:

- `manifest_phase`
- `run_id`
- `scope_root`
- `artifact_count`
- `total_size_bytes`
- `schema_coverage`
- `required_artifacts_missing`
- `retention_policy`
- `raw_jsonl_git_staging_allowed`
- `artifacts`

Required PR10 artifact entry fields:

- `relative_path`
- `size_bytes`
- `line_count`
- `sha256`
- `sha256_status`
- `jsonl_rows`
- `malformed_jsonl_rows`
- `schema_counts`
- `is_symlink`
- `status`

Required PR10 properties:

- `raw_jsonl_git_staging_allowed=false`;
- sha256 is recorded for feasible files or explicitly marked
  `SKIPPED_TOO_LARGE`;
- JSONL malformed rows are counted and block strict manifest acceptance;
- symlinks are not followed silently and are reported as `BLOCKED_SYMLINK`;
- required post-run artifacts are defined in
  `reports/selector/shadow_v2_manifest_artifact_contract.csv`;
- no cleanup is allowed before pre/post manifest evidence exists.

PR10 acceptance is limited to manifest contract readiness and deterministic
fixture coverage. It is not research-grade proof by itself because no validation
burnin evidence is collected in PR10.

## PR11 Logging-Only Burnin Config Contract

PR11 defines an inert configuration surface for a future Shadow V2 fidelity
validation burnin. The config is allowed to describe a logging-only validation
profile, but it must not enable any runtime behavior.

Required PR11 config record:

```text
shadow_v2_burnin
```

Required PR11 invariants:

- default config is disabled;
- enabled profiles must use `mode=logging_only_validation`;
- `logging_only=true`;
- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `strategy_proof_enabled=false`;
- `rce_proof_enabled=false`;
- `selector_proof_enabled=false`;
- `edge_proof_enabled=false`;
- `no_raw_jsonl_git_staging=true`;
- evidence manifest, sha256, row count and schema coverage requirements stay
  enabled;
- without PR14 live-confirmed calibration dataset, max verdict remains
  `SHADOW_V2_RESEARCH_GRADE_ONLY`.

PR11 does not connect the config to BUY/REJECT, Gatekeeper policy, selector
runtime, TX/Jito/live path, shadow close or active close.

## PR12 Boundary

PR12 is:

```text
Shadow V2 Fidelity Validation Burnin Plan
```

PR12 is not:

- strategy proof,
- RCE proof,
- selector proof,
- edge proof,
- runtime approval proof.

Success criteria are fidelity/reconciliation/density/manifest gates only.

PR12 does not start a validation burnin. It defines the static plan, required
artifacts, required horizons and required gates for a future fidelity-only
burnin. Any actual run requires separate operator approval after review.

Required PR12 static artifacts:

- `PLANS/AUDYT/PLAN_SHADOW_V2_FIDELITY_VALIDATION_BURNIN_PR12_20260630.md`
- `configs/rollout/shadow_v2_fidelity_validation_burnin_plan.toml`
- `scripts/shadow_v2_validation_burnin_plan_audit.py`
- `scripts/test_shadow_v2_validation_burnin_plan_audit.py`

PR12 plan invariants:

- `plan_status=PLAN_ONLY`;
- `validation_mode=FIDELITY_ONLY`;
- `run_start_allowed=false`;
- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- strategy/RCE/selector/edge proof flags remain false;
- R51 touch is not allowed;
- raw JSONL staging is not allowed;
- PR14 live-confirmed calibration is required before any live-equivalence grade.

## PR13 Boundary

PR13 is:

```text
Legacy adapter and downgrade enforcement
```

PR13 is not:

- deletion of V1 evidence,
- runtime migration,
- strategy proof,
- live-equivalence upgrade,
- RCE approval,
- selector approval,
- active close approval.

Required PR13 static artifacts:

- `PLANS/AUDYT/RAPORT_SHADOW_V2_LEGACY_DOWNGRADE_ENFORCEMENT_PR13_20260630.md`
- `reports/selector/shadow_v2_legacy_downgrade_matrix.csv`
- `scripts/shadow_v2_legacy_downgrade_audit.py`
- `scripts/test_shadow_v2_legacy_downgrade_audit.py`

PR13 downgrade invariants:

- V1 reports remain available only as downgraded evidence;
- V1 never live-equivalent;
- `shadow_exit_replay_v1` remains `MARK_PRICE_REPLAY_ONLY`;
- Shadow V1 lifecycle remains `LIFECYCLE_V1_NOT_CANONICAL`;
- R51 remains `ACTIVE_PARTIAL_DIAGNOSTIC_ONLY`;
- previous reports must not be cited as proof of live PnL, executable fills,
  live slippage behavior, real landing outcome, runtime approval, RCE approval,
  selector proof, `shadow_close_only` approval or active close approval.

## PR14 Boundary

PR14 is:

```text
Live-confirmed calibration dataset
```

PR14 is required for:

```text
SHADOW_V2_LIVE_EQUIVALENCE_GRADE
```

Without PR14, max verdict is:

```text
SHADOW_V2_RESEARCH_GRADE_ONLY
```

PR14 contract artifacts are:

```text
configs/rollout/shadow_v2_live_confirmed_calibration_contract.toml
reports/selector/shadow_v2_live_calibration_schema_manifest.csv
reports/selector/shadow_v2_live_calibration_gap_matrix.csv
scripts/shadow_v2_live_calibration_audit.py
scripts/test_shadow_v2_live_calibration_audit.py
```

PR14 required dataset files are:

```text
live_calibration_manifest.json
live_transaction_attempts.jsonl
live_confirmed_entry_fills.jsonl
live_confirmed_exit_fills.jsonl
live_calibration_comparison.jsonl
```

PR14 required schemas are:

- `live_calibration_manifest_v1`;
- `live_transaction_attempt_v1`;
- `live_confirmed_entry_fill_v1`;
- `live_confirmed_exit_fill_v1`;
- `live_calibration_comparison_v1`.

PR14 required telemetry includes:

- `decision_ts_ms`;
- `submit_ts_ms`;
- `landing_ts_ms`;
- `decision_to_submit_ms`;
- `submit_to_land_ms`;
- `landing_slot`;
- `fill_status`;
- `failure_mode`;
- `quote_price`;
- `fill_price`;
- `realized_slippage_bps`;
- `quote_fill_diff_bps`;
- `own_impact_bps`;
- `fee_bps`;
- `priority_fee_lamports`;
- `jito_tip_lamports`;
- `account_state_delay_ms`;
- `stream_delay_ms`;
- calibrated `model_error_bps`.

PR14 audit semantics:

- default audit validates contract readiness only;
- default audit must not read raw run JSONL or start runs;
- `--dataset-root <path> --require-dataset` is required for the real
  live-confirmed calibration gate;
- fixture datasets are not live-confirmed calibration evidence;
- `CONTRACT_READY` is not `SHADOW_V2_LIVE_EQUIVALENCE_GRADE`;
- real dataset absence keeps max verdict at `SHADOW_V2_RESEARCH_GRADE_ONLY`.

## PR15 Boundary

PR15 is:

```text
Shadow V2 Validation Execution Harness
```

PR15 is the first runtime-adjacent logging-only evidence producer for Shadow V2.
It remains disabled by default and must not change decision or execution
semantics.

PR15 is not:

- validation burnin execution;
- strategy proof;
- RCE proof;
- selector proof;
- edge proof;
- runtime approval;
- `shadow_close_only` approval;
- active close approval.

Required PR15 config fields when `shadow_v2_burnin.enabled=true`:

- `scope_root_path`;
- `pre_run_manifest_path`;
- `post_run_manifest_path`;
- `canonical_event_stream_path`;
- `replay_v2_path`;
- `lifecycle_v2_path`;
- `path_density_v2_path`.

Disabled behavior:

- `shadow_v2_burnin.enabled=false` preserves process startup and runtime
  behavior;
- no Shadow V2 validation harness is initialized;
- no manifest audit is invoked;
- no Shadow V2 artifact is consumed by decision or execution paths.

Enabled behavior:

- `[shadow_v2_burnin]` must be loaded by a partial Shadow V2 burnin config
  loader before the full Ghost Brain config fallback path;
- an unrelated full Ghost Brain config error must not silently turn
  `shadow_v2_burnin.enabled=true` into disabled/no harness;
- the validation harness may fail startup only as
  `SHADOW_V2_VALIDATION_PREFLIGHT_FAILED`;
- this failure is not a Gatekeeper failure, BUY/REJECT failure or selector
  failure;
- pre-run manifest audit runs with `--strict`;
- post-run manifest audit runs as a generation pass without `--strict`, followed
  by a separate strict verification pass.
- post-run generation writes both `post_run_manifest.json` and
  `shadow_v2_manifest_report.csv`;
- generation targets passed by `--write-manifest` and `--write-report-csv` are
  treated as artifacts produced in the same pass and must not create a
  self-blocked manifest.

Python manifest audit is allowed only at harness start and shutdown/post-run.
Python manifest audit must not run per event, per slot, per tx, per position
update or in the hot decision path.

PR15 canonical artifact:

```text
shadow_position_event_v2.jsonl
```

PR15 derived artifacts:

```text
shadow_replay_v2.jsonl
shadow_lifecycle_v2.jsonl
shadow_path_density_v2.jsonl
```

Derived replay/lifecycle rows are append-only snapshots keyed by canonical
high-watermark:

```text
replay_v2:{position_id}:{source_canonical_high_watermark}
lifecycle_v2:{position_id}:{source_canonical_high_watermark}
```

Derived artifacts are not canonical terminal truth and must include canonical
source refs.

`shadow_path_density_v2` rows must use a concrete wrapper schema with:

- schema identity;
- run/session/position/pool/mint identity;
- canonical event stream ref;
- source path sample event ids;
- source canonical high-watermark;
- horizon verdict;
- path point and coverage counts;
- interval and horizon metadata;
- duplicate/non-monotonic/truncation flags;
- limitations;
- creation wall-clock timestamp.

Bare `ShadowPathHorizonEvaluationV2` rows are not valid
`shadow_path_density_v2` JSONL records.

PR15 write outcome must distinguish:

- canonical durable write success/failure;
- replay derived write success/failure/skipped;
- lifecycle derived write success/failure/skipped;
- density write success/failure/skipped.

Canonical durable success must not be rolled back when a derived artifact write
fails. Instead the harness must emit a validation evidence status such as
`DERIVED_ARTIFACT_WRITE_FAILED` or `DENSITY_WRITE_FAILED`.

PR15 may emit a minimal diagnostic `shadow_position_v2` after accepted shadow
handoff. Such a record must stay:

- `simulation_level=MARK_ONLY`;
- `measurement_grade=DIAGNOSTIC_ONLY`;
- limitation `PR15_MINIMAL_POSITION_CREATED_ONLY`;
- limitation `NO_ENTRY_FILL_EXIT_FILL_OR_PATH_INFERENCE_IN_PR15`;
- limitation `SHADOW_V2_RECORD_NOT_CONSUMED_BY_DECISIONS`.

PR16A may emit one deterministic logging-only validation smoke marker at
Shadow V2 harness startup. This marker exists only to prove canonical writer,
derived replay/lifecycle, density rows and manifest wiring without waiting for a
random BUY / accepted shadow handoff. The marker must stay:

- active only when `shadow_v2_burnin.enabled=true`;
- active only when `shadow_v2_burnin.logging_only=true`;
- `simulation_level=MARK_ONLY`;
- `measurement_grade=DIAGNOSTIC_ONLY`;
- `temporal_class=UNKNOWN`;
- limitation `VALIDATION_SMOKE_MARKER_V2`;
- limitation `DIAGNOSTIC_ONLY_NOT_STRATEGY_POSITION`;
- limitation `BLOCKED_BY_DATA_NO_ENTRY_FILL_EXIT_FILL_OR_PATH`;
- limitation `NOT_CONSUMED_BY_DECISIONS`;
- limitation `NOT_STRATEGY_EVIDENCE`;
- limitation `NOT_LIVE_EQUIVALENT`.

The marker must not be consumed by Gatekeeper, selector, BUY/REJECT,
TX/Jito/live path, `shadow_close_only` or active close.

## Research-Grade Gates

`SHADOW_V2_RESEARCH_GRADE` requires:

- entry reconstruction coverage >= 99%,
- exit reconstruction coverage >= 99%,
- lifecycle/replay terminal reconciliation >= 99%,
- duplicate terminal records = 0 or typed sub-events,
- ambiguous fallback joins accepted silently = 0,
- critical temporal leakage = 0,
- timestamp clock-domain unknown = 0 for research fields,
- path density report for every horizon,
- unsupported horizons marked `NOT_EVALUABLE`,
- field registry complete,
- fixtures pass,
- golden traces inspectable,
- manifests complete.

## Live-Equivalence Gates

`SHADOW_V2_LIVE_EQUIVALENCE_GRADE` additionally requires:

- PR14 live-confirmed calibration dataset,
- latency model,
- landing/failure/no-fill model,
- entry slippage model,
- exit slippage model,
- own impact model,
- fee model,
- quote/fill divergence measurement/model,
- calibrated model error report,
- live-confirmed comparison,
- severe live-equivalence gaps = 0.

## Runtime Boundary

PR10/PR11 remain side-by-side Shadow V2 manifest/config contracts and fixtures
only. PR15 adds a disabled-by-default logging-only validation harness, but does
not grant strategy, research-grade or live-equivalence proof.

Forbidden in PR10/PR11/PR15:

- non-logging-only writer activation,
- any writer activation that affects BUY/REJECT, Gatekeeper, selector or
  execution behavior,
- runtime config consumption for execution,
- lifecycle behavior change,
- replay behavior change,
- manifest cleanup,
- BUY/REJECT change,
- selector runtime change,
- TX/Jito/live path change,
- `shadow_close_only` enablement,
- active close enablement,
- run start,
- R51 touch,
- strategy research unblocking.
