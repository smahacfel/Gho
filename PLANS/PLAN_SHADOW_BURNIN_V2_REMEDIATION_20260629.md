# Plan Shadow Burnin V2 Remediation 2026-06-29

## 1. Executive Summary

Bazowy werdykt pomiarowy pozostaje:

```text
SHADOW_REPLAY_LIFECYCLE_MISMATCH
```

Shadow V1 nie może być używany jako:

- unified position truth,
- live-equivalent PnL proof,
- proof of executable fills,
- proof of live slippage behavior,
- proof of real landing outcome.

Ten plan definiuje kompletną ścieżkę inżynieryjną do Shadow Burnin Simulation V2. V2 ma być dodatkiem side-by-side, domyślnie disabled, bez zmian BUY/REJECT, bez zmian Gatekeeper policy, bez zmian selector runtime, bez zmian TX/Jito/live path, bez włączania `shadow_close_only` i bez włączania active close.

Planistyczny werdykt:

```text
SHADOW_V2_REMEDIATION_PLAN_READY
```

Do czasu przejścia bramek V2:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
R52_approval=false
strategy_research_unblocked=false
```

R51 pozostaje:

```text
ACTIVE_PARTIAL / DIAGNOSTIC_ONLY
```

## 2. Root Evidence

Shadow V2 remediation opiera się na aktualnym P0 fidelity audit i downgrade pack. Jeżeli poniższe artefakty są niedostępne w worktree wykonawczym, remediation musi zostać oznaczona jako:

```text
BLOCKED_MISSING_FIDELITY_AUDIT_ARTIFACTS
```

Wymagane artefakty bazowe:

- `PLANS/AUDYT/RAPORT_SHADOW_BURNIN_FIDELITY_AUDIT_20260629.md`
- `docs/ADR/ADR_8D_SHADOW_BURNIN_FIDELITY_AUDIT_20260629.md`
- `reports/selector/shadow_fidelity_claim_evidence_matrix.csv`

Baseline audit stwierdził:

- entry price nie jest udowodniony jako executable live fill,
- exit result jest offline mark/path label evidence, nie executable sell fill,
- `shadow_lifecycle` i `shadow_exit_replay_v1` nie są jedną spójną prawdą pozycji,
- replay/lifecycle final PnL, close reason i close age materialnie się rozjeżdżają,
- 2s/3s wnioski są sparse approximation only,
- 300s/500s horizons nie są evaluable z obecnego replay horizon,
- latency, landing, failed tx/no-fill, slippage, own impact i quote/fill divergence nie są wystarczająco modelowane albo logowane.

## 3. Current Failure Statement

Shadow V1 mierzy głównie offline path/mark behavior. Nie mierzy wiarygodnie tego, czy live buy/sell byłby możliwy, po jakiej cenie zostałby zrealizowany, czy transakcja wylądowałaby na chainie, czy min-out/slippage by przeszły oraz jaki byłby realny PnL po fees, latency i own impact.

Najważniejszy błąd architektoniczny V1:

```text
shadow_lifecycle and shadow_exit_replay_v1 act as separate truths.
```

Shadow V2 musi usunąć ten problem przez jeden canonical event-sourced position truth.

## 4. Target Shadow V2 Architecture

Shadow V2 musi rozdzielać trzy warstwy pomiaru.

### MARK_PRICE_REPLAY

Offline path/mark evidence.

Dozwolone:

- labels,
- path-derived MFE/MAE,
- target/stop/timeout research labels,
- density/horizon analysis.

Zabronione:

- live-equivalent PnL claim,
- executable fill claim,
- landing claim,
- slippage claim bez modelu,
- no-fill/failure claim bez modelu.

### EXECUTABLE_FILL_SIM

Causal fill simulation using:

- pool state,
- event order key,
- clock-domain contract,
- latency model,
- slippage model,
- own-impact model,
- fee model,
- failure/no-fill model,
- quote/fill divergence model or measurement.

Research-grade jest możliwe tylko, gdy wszystkie wymagane inputy są obecne albo record jest jawnie `BLOCKED_BY_DATA`.

### LIVE_CONFIRMED

Actual landed transaction / confirmed fill evidence.

Ta warstwa:

- istnieje tylko dla real live trades,
- jest calibration data,
- nie może być zakładana przez shadow,
- jest wymagana do `SHADOW_V2_LIVE_EQUIVALENCE_GRADE`.

## 5. Current Code Map

| Obszar | Aktualny kod | Rola V1 | Rola V2 |
|---|---|---|---|
| Decision snapshot | `ghost-launcher/src/session/observation.rs::PoolObservationSession::materialize_features` | SSOT decyzji Gatekeeper | Źródło `shadow_entry_decision_v2`; bez post-entry/outcome leakage |
| Decision logging | `ghost-brain/src/oracle/decision_logger.rs` | Decision JSONL, `materialized_feature_snapshot`, selector score | Źródło decision evidence, field registry, temporal proof |
| Shadow entry | `ghost-launcher/src/oracle_runtime.rs::{shadow_entry_price, shadow_entry_record_from_event, shadow_entry_record_from_request, ShadowEntryRecord}` | Entry mark/derived price | Legacy adapter; V2 rozdziela decision price, quote price, fill price |
| Shadow backend | `ghost-brain/src/execution/shadow.rs::ShadowBackend` | `shadow_entries.jsonl` writer | Legacy source only; nie canonical truth |
| Lifecycle V1 | `ghost-brain/src/guardian/post_buy/engine.rs::{append_shadow_lifecycle_record, emit_shadow_exit, emit_position_closed}` | Oddzielna prawda lifecycle | Derived view z canonical V2 stream |
| Exit replay V1 | `ghost-brain/src/guardian/post_buy/exit_replay.rs::ShadowExitReplayTracker` | `path_bps`, `first_hit_ms`, MFE/MAE | Wzorzec MARK_PRICE_REPLAY, ale nie terminal truth |
| Price resolver | `ghost-brain/src/guardian/post_buy/engine.rs::PriceTruthResolver` | V1 shadow exit price resolver | V2 wymaga `pool_state_sample_v2` ref dla każdej ceny |
| Pool state | `ghost-core/src/account_state_core/types.rs`, `ghost-brain/src/oracle/snapshot_engine.rs` | AccountStateCore/SnapshotEngine/ShadowLedger evidence | AccountStateCore + stream/snapshot as source; ShadowLedger diagnostic only |
| Post-buy handoff | `ghost-launcher/src/components/post_buy_runtime.rs::ShadowPostBuyHandoffResult` | Shadow handoff to monitor | Boundary dla future V2 event writer |
| Live execution | `ghost-launcher/src/components/trigger/component.rs`, `ghost-launcher/src/components/live_tx_sender.rs`, `off-chain/components/trigger/*` | TX/Jito/live path | Nie ruszać; future calibration source dla `LIVE_CONFIRMED` |

## 6. Single Source Of Truth V2

Shadow V2 musi mieć jeden canonical append-only stream:

```text
shadow_position_event_v2.jsonl
```

Z niego wolno generować derived artifacts:

- `shadow_lifecycle_v2.jsonl`
- `shadow_replay_v2.jsonl`
- `shadow_terminal_truth_v2.jsonl`
- reports/fidelity/density/golden traces

Reguły:

- derived artifact nie może być competing truth,
- dokładnie jeden canonical terminal event per `position_id`,
- `exit_filled` i `position_closed` mogą istnieć tylko jako typed sub-events albo derived views,
- no silent fallback joins,
- no ambiguous fallback joins,
- każdy derived row musi wskazywać `event_id`, `position_id`, `source_event_id` i `canonical_terminal_event_id`, jeżeli dotyczy.

## 7. Common V2 Envelope

Każdy rekord V2 musi mieć wspólny envelope:

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

## 8. Canonical Entities

V2 musi zdefiniować co najmniej:

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

Każda encja ma być schema-versioned, additive i replay/audit friendly.

## 9. Event Order Key

Następujące rekordy muszą zawierać `event_order_key`:

- `pool_state_sample_v2`
- `shadow_path_sample_v2`
- `shadow_entry_attempt_v2`
- `shadow_entry_fill_v2`
- `shadow_exit_attempt_v2`
- `shadow_exit_fill_v2`

`event_order_key` fields:

- `slot`
- `block_time`
- `signature`
- `transaction_index_or_unknown`
- `instruction_index_or_unknown`
- `inner_instruction_index_or_unknown`
- `log_index_or_unknown`
- `event_seq_in_process`
- `observed_at_wall_ms`

Reguły:

- `event_order_key` jest wymagany dla causal ordering.
- Brak elementu chain-order nie może być pusty; musi być explicit `UNKNOWN`.
- `event_seq_in_process` musi być monotoniczny w obrębie procesu i runu.
- Jeśli slot jest równy, a indeksy są niepełne, wynik musi dostać ambiguity label.
- Same-slot ambiguity nie może być deterministycznie traktowana jako win/loss bez policy.

## 10. Clock-Domain Contract

Każde pole timestamp musi deklarować clock domain.

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

Schema manifest musi dla każdego timestamp field zawierać:

- `timestamp_field_name`
- `clock_domain`
- `clock_source`
- `allowed_temporal_class`
- `causal_boundary`
- `missing_policy`

Reguły:

- Nie wolno porównywać clock domains bez jawnej reguły konwersji.
- `decision_ts_ms`, `submit_ts_ms`, `landing_ts_ms` są event semantics, nie ogólnym wall clock.
- `monotonic_process_ms` służy do ordering/latency within process, nie do chain truth.
- `block_time` może być missing albo coarse; nie może zastępować `observed_at_wall_ms`.
- `stream_observed_ms` i `rpc_observed_ms` muszą pozostać rozdzielone.
- Timestamp bez domeny blokuje research-grade.

Failure verdict:

```text
BLOCKED_TIMESTAMP_CLOCK_DOMAIN_UNKNOWN
```

## 11. Entry Price Remediation

V2 musi rozdzielić:

- `decision_mark_price`
- `entry_quote_price`
- `entry_fill_price`

Entry attempt/fill musi mieć:

- `event_order_key`
- clock-domain declarations,
- refs do `pool_state_sample_v2`,
- state phase: `PRE_DECISION`, `AT_DECISION`, `AT_SUBMIT`, `AT_LANDING`, `POST_LANDING`,
- own buy impact,
- slippage,
- fees,
- min-out,
- latency/landing model,
- failure/no-fill classification.

Entry fill status:

- `FILLED`
- `NO_FILL`
- `FAILED`
- `BLOCKED_BY_DATA`

Research-grade acceptance:

- entry reconstruction coverage >= 99%,
- diff <= agreed tolerance,
- every blocked record has typed reason,
- no price without slot/time/source/order key,
- no post-decision state accepted as pre-decision state.

Live-equivalence acceptance:

- blocked without latency,
- blocked without landing,
- blocked without entry slippage,
- blocked without own buy impact,
- blocked without fees,
- blocked without failure/no-fill model.

## 12. Exit Price Remediation

V2 musi rozdzielić:

- exact-level hit,
- sampled-path hit,
- mark exit,
- executable sell fill.

Exit attempt/fill musi mieć:

- `event_order_key`,
- clock-domain declarations,
- refs do `pool_state_sample_v2`,
- own sell impact,
- exit slippage,
- fees,
- min-out,
- no-fill/failure model,
- executable quote/fill model.

Same-slot target/stop:

- `same_slot_ambiguity=true`,
- explicit `tie_break_policy`,
- no deterministic inference without order evidence.

Timeout:

- timeout PnL musi wskazywać real path point albo stale/blocked status.

Research-grade acceptance:

- exit reconstruction coverage >= 99%,
- MFE/MAE/terminal PnL odtwarzalne z path,
- first-hit consistent with path or exact evidence,
- 2s/3s tylko gdy density supports,
- 300s/500s tylko gdy horizon supports.

## 13. Pool-State Provenance

Każda cena w V2 musi wskazywać `pool_state_sample_v2`.

`pool_state_sample_v2` musi logować:

- pool identity,
- base mint,
- bonding curve,
- observed wall time,
- slot,
- block time,
- source,
- commitment,
- event signature/index,
- account data hash,
- virtual reserves,
- real reserves,
- decimals,
- lamports normalization,
- price,
- market cap,
- bonding progress,
- source quality,
- staleness ms/slots,
- `event_order_key`.

Source enum:

- `YELLOWSTONE_EVENT`
- `ACCOUNT_STATE_CORE`
- `RPC_FALLBACK`
- `SHADOW_LEDGER_DIAGNOSTIC`
- `RECONSTRUCTED_FROM_TRADE_EVENT`
- `UNKNOWN`

Reguły:

- RPC fallback is never silently equivalent to stream state.
- ShadowLedger is never live truth.
- Stale samples are marked, not silently accepted.
- Sample without timestamp and slot is blocked for research-grade.

## 14. Temporal Integrity

Każde pole musi mieć temporal class:

- `PRE_DETECTION`
- `PRE_DECISION`
- `AT_DECISION`
- `POST_ENTRY`
- `POST_EXIT`
- `OUTCOME`
- `UNKNOWN`

Rules:

- selection features may only use `PRE_DECISION` / `AT_DECISION`,
- entry simulation may only use state available up to intended landing boundary,
- exit simulation may only use state available up to exit attempt boundary,
- outcome fields cannot be used as features,
- `UNKNOWN` fields block strategy research.

Critical failure:

```text
SHADOW_TEMPORAL_LEAKAGE_RISK
```

## 15. Path Density And Horizon Contract

Modes:

### `shadow_path_dense_3s`

Purpose:

- 2s/3s research.

Sampling:

- every trade/event sample,
- level-hit samples,
- terminal sample,
- ambiguity metadata.

Required before enabling:

- storage budget estimate from observed event rate.

### `shadow_path_standard_120s`

Purpose:

- 10s/30s/120s research.

Sampling:

- event samples,
- heartbeat,
- large price delta,
- level hit,
- terminal.

### `shadow_path_long_500s`

Purpose:

- 300s/500s research only when configured horizon and storage budget support it.

Sampling:

- compressed long path,
- explicit truncation metadata,
- terminal and large-delta preservation.

Unsupported horizon verdicts:

- `NOT_EVALUABLE_NO_COVERAGE`
- `NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY`
- `SPARSE_APPROX_ONLY`
- `EVALUABLE_APPROX`
- `EVALUABLE_EXACT`

No 300s/500s inference is allowed without actual coverage.

## 16. Live-Equivalence Model

Levels:

### Level 0: `MARK_ONLY`

- price path only,
- no fill claims.

### Level 1: `FILL_MODEL_STATIC`

- fixed latency/slippage/fee assumptions,
- max verdict: research-grade candidate,
- not live-equivalence-grade.

### Level 2: `FILL_MODEL_CALIBRATED`

- calibrated from live-confirmed/paper-live telemetry,
- required for live-equivalence candidate.

### Level 3: `LIVE_CONFIRMED`

- actual landed transaction evidence,
- calibration truth,
- never assumed by shadow.

Required telemetry for live-equivalence:

- decision-to-submit latency,
- submit-to-land latency,
- landing slot,
- priority fee,
- Jito tip,
- bundle success/failure,
- failed transaction,
- no-fill,
- compute failure,
- min-out failure,
- realized slippage,
- quote/fill divergence,
- own impact,
- account-state delay,
- stream delay,
- RPC delay.

Shadow V2 must never silently assume:

- zero latency,
- zero slippage,
- zero own impact,
- guaranteed fill.

## 17. Work Breakdown

| PR | Cel | Główne zmiany | Acceptance | Granice |
|---|---|---|---|---|
| PR0 | Freeze truth | P0 fidelity audit + downgrade pack | Baseline `SHADOW_REPLAY_LIFECYCLE_MISMATCH` | Już wykonane; nie powtarzać bez dyspozycji |
| PR1 | Contracts and schemas | Spec, schema manifest, envelope, `simulation_level`, `measurement_grade`, clock-domain registry, V2 Rust types | Serialization/schema completeness tests | Runtime disabled, no BUY/REJECT |
| PR2 | Canonical identity and terminal truth | `position_id`, `event_id`, canonical event stream, one terminal truth | Duplicate terminal tests, no ambiguous fallback join | V1 stays legacy |
| PR3 | Pool state provenance recorder | `pool_state_sample_v2`, `event_order_key`, source/slot/time/hash/staleness | Staleness/source/order tests | ShadowLedger diagnostic only |
| PR4 | Deterministic price reconstruction library | Bonding curve/AMM formulas, decimals/lamports, fees, quote helpers | Formula/rounding/fee tests | Runtime functions cannot be sole proof |
| PR5 | Entry executable fill model | Decision price, quote price, fill price, latency, own buy impact, slippage, fee, no-fill/failure | Entry fixtures, stale/future state blocks | No live buy path changes |
| PR6 | Exit executable fill model | Mark exit vs executable sell fill, own sell impact, slippage, fees, no-fill/failure | Exit fixtures, same-slot ambiguity, timeout source | No active close |
| PR7 | Path sampler V2 | `shadow_path_dense_3s`, `shadow_path_standard_120s`, `shadow_path_long_500s` | Density report, unsupported horizons `NOT_EVALUABLE` | Long mode needs storage budget |
| PR8 | Replay V2 derived view | `shadow_replay_v2` from canonical stream; mark/executable lanes separate | Reconstruction >= 99% | Replay not competing truth |
| PR9 | Lifecycle V2 derived view | `shadow_lifecycle_v2` from canonical stream | Lifecycle/replay reconciliation >= 99% | Duplicate terminal rows forbidden |
| PR10 | Evidence manifests and retention | pre/post manifests, sha256, row counts, schema coverage | Manifest completeness checks | No raw JSONL/log staging |
| PR11 | Shadow V2 burnin config | Logging-only V2 validation config, disabled default, serde defaults | Config compatibility tests | No runtime approval |
| PR12 | Shadow V2 Fidelity Validation Burnin Plan | Fidelity-only burnin plan proving reconciliation/density/manifests | Success = fidelity gates only | Not strategy proof, not RCE proof, not selector proof, not edge proof |
| PR13 | Legacy adapter and downgrade enforcement | V1 reports labelled mark/path-only; downgrade matrix enforced | Reader compatibility and downgrade tests | V1 never live-equivalent |
| PR14 | Live-confirmed calibration dataset | Dataset schema and ingestion for real landed/confirmed fills, latency, failures, slippage, quote/fill divergence | Calibration error report, live-confirmed comparison | Required for `SHADOW_V2_LIVE_EQUIVALENCE_GRADE` |

## 18. PR12 Boundary

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

PR12 success criteria are only:

- fidelity gates,
- reconciliation gates,
- density gates,
- manifest gates,
- schema coverage gates,
- golden trace inspectability.

## 19. PR14 Live-Confirmed Calibration Dataset

PR14 is required for:

```text
SHADOW_V2_LIVE_EQUIVALENCE_GRADE
```

Without PR14, max verdict is:

```text
SHADOW_V2_RESEARCH_GRADE_ONLY
```

PR14 must provide:

- live-confirmed entry fill telemetry,
- live-confirmed exit fill telemetry,
- decision-to-submit latency,
- submit-to-land latency,
- landing slot,
- failed landing,
- no-fill,
- realized entry slippage,
- realized exit slippage,
- own buy impact,
- own sell impact,
- fee/tip/priority fee evidence,
- quote/fill divergence,
- account-state delay,
- stream/RPC delay,
- calibrated model error report.

## 20. Testing Strategy

Rust unit tests:

- AMM/bonding curve price formula,
- entry fill formula,
- exit fill formula,
- decimals/lamports normalization,
- own buy impact,
- own sell impact,
- fee/slippage,
- state staleness guard,
- temporal class guard,
- clock-domain guard,
- event order key ordering,
- terminal truth invariant,
- no duplicate terminal rows,
- no ambiguous fallback join.

Rust integration tests:

- synthetic pool with deterministic trades,
- entry decision,
- simulated entry fill,
- path samples,
- target hit,
- stop hit,
- timeout,
- same-slot ambiguity,
- failed fill,
- no-fill,
- stale state blocked.

Python audit tests:

- independent reconstruction,
- fixture replay,
- lifecycle/replay reconciliation,
- path density support,
- live-equivalence gap.

Golden traces:

- 5 clean winners,
- 5 clean losers,
- 5 timeouts,
- 5 same-slot ambiguous,
- 5 stale/blocked,
- 5 no-fill/failure.

Future implementation commands:

```bash
cargo test -p ghost-core shadow_v2
cargo test -p ghost-brain shadow_v2
cargo test -p ghost-launcher shadow_v2
```

Python commands will be defined once V2 audit scripts exist.

## 21. Research-Grade Acceptance Gates

Verdict:

```text
SHADOW_V2_RESEARCH_GRADE
```

Minimum gates:

- entry price reconstruction coverage >= 99%,
- exit reconstruction coverage >= 99%,
- lifecycle/replay terminal reconciliation >= 99%,
- duplicate terminal records = 0 or explicitly typed sub-events,
- ambiguous fallback joins accepted silently = 0,
- critical temporal leakage findings = 0,
- timestamp clock-domain unknown = 0 for research fields,
- path density report generated for every horizon,
- unsupported horizons marked `NOT_EVALUABLE`,
- field registry complete for strategy-used fields,
- all simulator fixtures pass,
- golden traces manually inspectable,
- raw evidence manifests complete.

## 22. Live-Equivalence-Grade Acceptance Gates

Verdict:

```text
SHADOW_V2_LIVE_EQUIVALENCE_GRADE
```

Additional gates:

- PR14 live-confirmed calibration dataset exists,
- latency model present,
- landing/failure/no-fill model present,
- entry slippage model present,
- exit slippage model present,
- own impact model present,
- fee model present,
- quote/fill divergence measured or modeled,
- calibrated model error reported,
- live-confirmed sample comparison exists,
- severe live-equivalence gaps = 0.

If these gates are not met:

```text
SHADOW_V2_RESEARCH_GRADE_ONLY
```

## 23. Legacy Downgrade Matrix

Old reports must be downgraded from:

- live-equivalent PnL proof,
- executable fill proof,
- live slippage behavior proof,
- real landing outcome proof,
- unified lifecycle/replay truth.

Allowed use:

- offline mark/path label evidence,
- diagnostic-only evidence,
- component replay evidence with limitations.

Blocked use:

- runtime promotion,
- RCE approval,
- active close approval,
- `shadow_close_only` approval,
- R52 approval,
- live PnL proof.

Affected families:

- ORG-A0,
- R48/R2 exit matrix,
- TSV2 A1/A2/A3,
- EIX,
- RTP-A0,
- RUG-MARKUP-A0,
- RCE-A0,
- R51 partial outputs.

## 24. Required CSV Artifacts

### `reports/selector/shadow_v2_remediation_workbreakdown.csv`

Columns:

- `pr_id`
- `title`
- `purpose`
- `likely_files`
- `runtime_risk`
- `test_commands`
- `acceptance_gates`
- `rollback_plan`
- `evidence_generated`
- `explicit_non_changes`
- `status`

### `reports/selector/shadow_v2_required_schema_manifest.csv`

Columns:

- `record_name`
- `field_name`
- `type`
- `required_for_mark`
- `required_for_research_grade`
- `required_for_live_equivalence`
- `simulation_level`
- `measurement_grade`
- `temporal_class`
- `clock_domain`
- `source_owner`
- `nullable`
- `quality_rule`
- `notes`

### `reports/selector/shadow_v2_acceptance_gates.csv`

Columns:

- `gate_id`
- `grade`
- `metric`
- `threshold`
- `evidence_artifact`
- `test_command`
- `failure_verdict`
- `notes`

### `reports/selector/shadow_v2_legacy_downgrade_matrix.csv`

Columns:

- `report_family`
- `downgraded_from`
- `allowed_use`
- `blocked_use`
- `required_label`
- `upgrade_condition`

### `reports/selector/shadow_v2_risk_register.csv`

Columns:

- `risk_id`
- `risk`
- `severity`
- `impacted_pr`
- `mitigation`
- `acceptance_gate`
- `residual_status`

## 25. Required Companion Documents

The full remediation pack should later include:

- `docs/ADR/ADR_8D_SHADOW_BURNIN_V2_REMEDIATION_20260629.md`
- `docs/SPEC/SHADOW_BURNIN_V2_SIMULATION_CONTRACT_20260629.md`

ADR must include:

- decision,
- status,
- context,
- rejected alternatives,
- consequences,
- invariants,
- acceptance gates,
- no-runtime boundary.

Rejected alternatives:

- fixing V1 in-place,
- treating replay as terminal truth,
- treating lifecycle as terminal truth,
- assuming zero latency/zero slippage/zero own impact,
- promoting ShadowLedger to canonical truth,
- calling static fill assumptions live-equivalent.

## 26. Validation For Planning Artifacts

After materializing docs/CSV files:

```bash
git diff --check -- <changed files>
git diff --cached --name-only
```

Trailing whitespace check must pass.

Do not stage unless explicitly instructed.

Forbidden staged files:

- raw `.jsonl`,
- `logs/`,
- `runtime.log`,
- `datasets/events`,
- `__pycache__`,
- `shadow_lifecycle`,
- `shadow_exit_replay`,
- `gatekeeper_v2_decisions`,
- active R51 artifacts.

## 27. Non-Negotiable Runtime Boundary

This remediation planning task does not permit:

- runtime changes,
- run start,
- R51 touch,
- strategy research unblocking,
- BUY/REJECT changes,
- Gatekeeper policy changes,
- selector runtime changes,
- TX/Jito/live path changes,
- `shadow_close_only` enablement,
- active close enablement,
- cleanup,
- raw JSONL/log staging,
- `git add -A`.

## 28. Delegation Trace

```yaml
delegation_trace:
  task_classification: "cross_cutting_shadow_simulation_v2_planning"
  routing_performed: true
  primary_specialist: "ghost-runtime-coordinator"
  supporting_specialists_considered:
    - "decision-logging-replay-analyst"
    - "ssot-feature-materialization-guardian"
    - "solana-execution-path-engineer"
    - "oracle-session-runtime-engineer"
    - "config-rollout-safety-reviewer"
  specialist_docs_loaded:
    - "docs/agents/ghost-runtime-coordinator.md"
    - "docs/agents/decision-logging-replay-analyst.md"
    - "docs/agents/ssot-feature-materialization-guardian.md"
    - "docs/agents/solana-execution-path-engineer.md"
  skills_used:
    - "ghost-execution"
    - "trading-systems"
    - "rust-master"
    - "solana-pumpfun-architect"
    - "abstract-reasoning"
  fast_path_used: false
  contracts_checked:
    - "MaterializedFeatureSet SSOT"
    - "DecisionLogger/replay auditability"
    - "shadow/live boundary"
    - "AccountStateCore authority"
    - "ShadowLedger diagnostic-only boundary"
    - "terminal truth uniqueness"
    - "clock-domain integrity"
    - "event ordering integrity"
    - "config backward compatibility"
    - "no runtime approval from Shadow V1"
  unresolved_routing_uncertainty: []
```
