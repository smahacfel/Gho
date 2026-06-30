# Shadow Burnin V2 Simulation Contract 2026-06-29

## Status

```text
PR1_CONTRACT_DEFINED
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

PR1-level Rust contract types include `ShadowPositionV2` and the schema vocabulary for all records above. PR2 owns durable canonical writer/indexing invariants for `shadow_position_event_v2.jsonl`, including duplicate event and duplicate terminal rejection.

The canonical event stream is:

```text
shadow_position_event_v2.jsonl
```

Derived outputs must point back to canonical `event_id` and must not become competing truths.

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

## Terminal Truth Contract

Each `position_id` may have exactly one canonical terminal event.

Duplicate terminal lifecycle rows are allowed only as typed sub-events or derived views. Silent duplicate terminal truth is invalid.

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

PR1 is schema and contract only.

Forbidden in PR1:

- runtime writer activation,
- lifecycle behavior change,
- replay behavior change,
- BUY/REJECT change,
- selector runtime change,
- TX/Jito/live path change,
- `shadow_close_only` enablement,
- active close enablement,
- run start,
- R51 touch,
- strategy research unblocking.
