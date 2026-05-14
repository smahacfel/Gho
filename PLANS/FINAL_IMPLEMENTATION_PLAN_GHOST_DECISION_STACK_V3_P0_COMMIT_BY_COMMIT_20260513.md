# Final Implementation Plan: Ghost Decision Stack V3 P0

Status: verified planning document only. No implementation, no patches, no commits.

Verified against local repository: `/root/Gho`

Verified HEAD: `d96aba8`

Date: 2026-05-13

Source plan: `PLANS/PLAN_WYKONAWCZY_GHOST_DECISION_STACK_V3_P0_20260513.md`

Primary constraint: P0 is shadow/evidence-plane only. V3 must not change active BUY, REJECT, TIMEOUT, IWIM, execution, blockhash, retry, confirmation, or live/shadow semantics.

---

## 1. Repo Verification Summary

The plan is broadly compatible with the current repository, with one important correction around logging plane semantics.

Verified current anchors:

- `MaterializedFeatureSet` still lives in `ghost-core/src/checkpoint/types.rs`.
- `MaterializedFeatureSet` is still the canonical decision snapshot for Gatekeeper-facing evaluation.
- `PoolObservationSession::materialize_features()` in `ghost-launcher/src/session/observation.rs` is still the correct SSOT boundary for turning mutable runtime/session state into immutable decision evidence.
- `ghost-core/src/checkpoint/mod.rs` uses explicit re-exports, so any new public V3 evidence/materialization types must be added there.
- `ObservationFeatureBuilder::materialize()` in `ghost-core/src/checkpoint/feature_builder.rs` manually constructs `MaterializedFeatureSet`, so adding fields requires updating this constructor and related tests.
- `GatekeeperReasonCode` still lives in `ghost-brain/src/oracle/reason_code.rs`.
- `GatekeeperBuyLog` and `GATEKEEPER_BUY_LOG_SCHEMA_VERSION` still live in `ghost-brain/src/oracle/decision_logger.rs`.
- `ghost-launcher` can depend on and import `ghost-brain`; `ghost-brain` must not take a production dependency on `ghost-launcher`. Current `ghost-brain -> ghost-launcher` usage is dev-only.
- `GatekeeperBuffer::evaluate_from_features()` in `ghost-launcher/src/components/gatekeeper.rs` is still the active feature-driven evaluation path and must remain semantically unchanged.
- The safest V3 runtime hook is after `assessment.to_buy_log(...)`, using `assessment.feature_snapshot`, so V3 results enrich logs only and do not feed active verdict selection.
- The existing logger already expands `legacy_live` and `v25_shadow` planes. P0 should not add a separate routed `v3_shadow` decision plane unless the writer semantics are redesigned.

Important active path classification:

- `GatekeeperBuffer::evaluate_from_features()`: active policy path, do not change.
- `PoolObservationSession::materialize_features()`: active materialization boundary, may add additive evidence fields.
- V3 evaluator: new shadow/evidence-only path.
- V3 log fields: durable diagnostic/audit fields only.
- V3 report script: offline analytical tooling only.

---

## 2. Potential Mismatches Between Plan And Current Code

The existing plan should be adjusted before implementation in these places:

1. Do not create a separate routed `v3_shadow` plane in P0.

   Current `DecisionLogger` plane expansion is designed around `legacy_live` and `v25_shadow`. If a separate `v3_shadow` plane were written as a normal decision row, a V3 shadow BUY could be accidentally routed into buy-oriented outputs unless logger semantics are changed carefully. For P0, V3 should be stored as additive `v3_shadow_*` fields on existing rows.

2. Adding fields to `MaterializedFeatureSet` requires more than serde defaults.

   Current code has Rust struct literals and builders that manually construct `MaterializedFeatureSet`. All such constructors/tests must receive explicit default V3 values.

3. Evidence defaults must fail conservative.

   Missing or degraded evidence cannot default to clean. New evidence status types must default to unavailable/unknown/degraded-safe states.

4. V3 Pending is not active PendingCurve.

   V3 can emit a shadow Pending/InsufficientEvidence-style outcome for analysis, but it must not affect active terminal verdicts or be treated as current active curve readiness policy.

5. Runtime hook must avoid re-materialization.

   The V3 shadow evaluator should consume `assessment.feature_snapshot`. Recalling `materialize_features()` later in runtime/logging would risk dual-authority feature snapshots.

6. Script/reporting must deduplicate expanded logger rows.

   Since current logging may include both `legacy_live` and `v25_shadow` rows for one decision, the V3 report script must avoid double-counting.

7. No execution evidence shortcut.

   P0 must not infer execution success. Execution-related V3 evidence should be `execution_not_run`, `unavailable`, or equivalent conservative status.

---

## 3. Final Implementation Sequence

Use six narrow commits:

1. Types only.
2. Materialization only.
3. Reason codes and logger schema only.
4. V3 shadow evaluator only.
5. Runtime/log integration only.
6. V3 shadow report script only.

This sequence keeps the SSOT contract intact and isolates behavior risk:

- Commit 1 makes types compile and keeps old serialized data compatible.
- Commit 2 makes V3 evidence materialized at the canonical boundary.
- Commit 3 makes logs and reason taxonomy capable of storing V3 diagnostics.
- Commit 4 implements pure shadow evaluation with no runtime side effects.
- Commit 5 wires V3 into logging only.
- Commit 6 adds offline analysis without runtime changes.

---

## 4. Commit 1: Types Only

### Files

- `ghost-core/src/checkpoint/types.rs`
- `ghost-core/src/checkpoint/mod.rs`
- `ghost-core/src/checkpoint/feature_builder.rs`
- Existing tests with `MaterializedFeatureSet` struct literals, likely including:
  - `ghost-core/tests/pr1_contracts_foundations.rs`
  - `ghost-launcher/tests/gatekeeper_policy_tests.rs`
  - `ghost-launcher/tests/full_pipeline_integration.rs`

### Exact Intended Changes

Add V3 evidence model types in `ghost-core/src/checkpoint/types.rs`:

- `EvidenceStatus`
  - conservative default: unavailable/unknown, not clean
  - no global `is_actionable()` helper
- `EvidenceDegradedReason`
- `EvidenceUnavailableReason`
- `FeatureEvidenceStatus`
- `MaterializedEvidenceStatus`
- `OrganicBroadeningFeatures`
- `ManipulationContradictionFeatures`

Extend `MaterializedFeatureSet` with additive fields:

- `evidence_status: MaterializedEvidenceStatus`
- `organic_broadening: OrganicBroadeningFeatures`
- `manipulation_contradictions: ManipulationContradictionFeatures`

All new fields must use backward-compatible serde defaults where old JSON/log fixtures/config-like serialized data must still load.

Update `ObservationFeatureBuilder::materialize()` so it initializes the new fields with conservative defaults.

Update `ghost-core/src/checkpoint/mod.rs` explicit re-exports for any new public types that launcher/tests need.

Update existing `MaterializedFeatureSet` literals in tests to include explicit V3 defaults or use helper constructors if available.

### Tests

Targeted tests to add or extend:

- Existing feature-builder materialization test should assert new fields are present and conservatively defaulted.
- Existing materialized snapshot contract test should assert `MaterializedFeatureSet` contains complete input domains plus V3 evidence fields.
- Add JSON backward-compatibility roundtrip:
  - deserialize old `MaterializedFeatureSet` JSON without V3 fields
  - assert new fields default to unavailable/not-clean

Suggested commands:

```bash
cargo test -p ghost-core feature_builder
cargo test -p ghost-core materialized
```

### Acceptance Criteria

- `ghost-core` compiles.
- Old serialized `MaterializedFeatureSet` data without V3 fields still deserializes.
- Missing V3 evidence defaults to unavailable/degraded-safe, not clean.
- No `ghost-core` dependency on `ghost-launcher` or `ghost-brain`.
- No active Gatekeeper behavior changes.

---

## 5. Commit 2: Materialization Only

### Files

- `ghost-launcher/src/session/observation.rs`
- Existing session/materialization tests, likely including:
  - `ghost-launcher/tests/session_lifecycle_tests.rs`
  - `ghost-launcher/tests/gatekeeper_v25_regression.rs`

### Exact Intended Changes

Add private materialization helpers inside the observation/session materialization area:

- `materialize_v3_evidence_status(...)`
- `materialize_v3_organic_broadening(...)`
- `materialize_v3_manipulation_contradictions(...)`

These helpers must only be called from `PoolObservationSession::materialize_features()`.

Use already-owned materialized/session inputs:

- existing `tx_segment_sequence`
- existing `TxIntelFeatures`
- existing `SybilResistanceFeatures`
- existing alpha/prosperity/checkpoint/curve readiness evidence
- session-owned `tx_buffer` only inside materialization, if needed to compute segment-level unique signer shape

Do not read runtime state from policy evaluation.

Suggested V3 materialization behavior:

- Compute organic broadening from T0/T1/T2 transaction shape, unique signer broadening, buy participation, and HHI-like concentration metrics where current data supports it.
- Compute manipulation contradictions from bundle/same-ms/concentration/top-holder/sybil/curve contradictions already available in materialized features.
- Mark missing segment data as unavailable or degraded, not clean.
- Mark missing alpha/sybil/CPV/FSC inputs as degraded/unavailable using explicit reasons.
- Mark execution evidence as not run/unavailable for P0.

### Tests

Add/extend tests for:

- Full T0/T1/T2 data produces deterministic organic broadening fields.
- Missing segment data produces non-clean evidence status.
- Sybil degraded reasons map into V3 evidence status.
- Missing alpha/prosperity data does not become clean evidence.
- Existing active V2/V2.5 verdict tests still pass unchanged.

Suggested commands:

```bash
cargo test -p ghost-launcher session_lifecycle
cargo test -p ghost-launcher --test gatekeeper_v25_regression
```

### Acceptance Criteria

- All V3 evidence is materialized at `PoolObservationSession::materialize_features()`.
- No V3 feature is recomputed inside active policy evaluation.
- `GatekeeperBuffer::evaluate_from_features()` result is unchanged for existing fixtures.
- No DirectBuy/DirectSell/LiveTxSender/Helius/blockhash/retry/confirmation code touched.

---

## 6. Commit 3: Reason Codes And Logger Schema Only

### Files

- `ghost-brain/src/oracle/reason_code.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- Existing logger/reason-code tests in `ghost-brain`

### Exact Intended Changes

In `reason_code.rs`:

- Add V3 P0 shadow/evidence reason codes needed for V3 diagnostics.
- Keep reason codes typed and explicit.
- Bump reason-code taxonomy/schema version from current version to the next version.

Possible reason-code classes:

- V3 evidence unavailable
- V3 evidence degraded
- V3 organic broadening insufficient
- V3 manipulation contradiction
- V3 hard risk reject
- V3 shadow buy candidate
- V3 shadow pending / insufficient evidence
- V3 shadow timeout evidence

In `decision_logger.rs`:

- Bump `GATEKEEPER_BUY_LOG_SCHEMA_VERSION` by one.
- Add additive optional `v3_shadow_*` fields to `GatekeeperBuyLog`.
- Use serde defaults/skip behavior compatible with old rows.
- Keep old rows readable.
- Keep existing `legacy_live` and `v25_shadow` plane expansion semantics intact.
- Do not add a separate routed `v3_shadow` plane in P0.

Recommended `GatekeeperBuyLog` additive fields:

- `v3_shadow_schema_version`
- `v3_shadow_verdict`
- `v3_shadow_reason_code`
- `v3_shadow_reason_chain`
- `v3_shadow_confidence`
- `v3_shadow_evidence_status`
- `v3_shadow_organic_broadening`
- `v3_shadow_manipulation_contradictions`
- `v3_shadow_notes` or compact diagnostics object if needed

Use structured JSON fields only where preserving nested evidence is materially useful for offline analysis.

### Tests

Add/extend tests for:

- New reason codes serialize and parse roundtrip.
- `GatekeeperReasonCode::from_log_str(...)` recognizes new V3 codes.
- Old v19/vcurrent `GatekeeperBuyLog` JSON without V3 fields deserializes.
- New log rows with V3 fields serialize without dropping required active reason code.
- Existing logger plane expansion tests still pass.

Suggested commands:

```bash
cargo test -p ghost-brain reason_code
cargo test -p ghost-brain decision_logger
```

### Acceptance Criteria

- Logger schema change is additive.
- No old required field becomes optional in a way that weakens auditability.
- Missing active `reason_code` behavior remains unchanged.
- V3 fields do not alter `decision_verdict_buy`, active verdict, active reason code, or buy routing.
- No production `ghost-brain -> ghost-launcher` dependency is introduced.

---

## 7. Commit 4: V3 Shadow Evaluator Only

### Files

- New file: `ghost-launcher/src/components/gatekeeper_v3.rs`
- `ghost-launcher/src/components/mod.rs`
- New or existing launcher tests, likely:
  - `ghost-launcher/tests/gatekeeper_v3_tests.rs`
  - or a focused module test near component policy tests

### Exact Intended Changes

Create a pure V3 shadow evaluator:

```text
evaluate_v3_from_features(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV2Config,
    deadline_elapsed: bool,
) -> V3ShadowDecision
```

The exact Rust signature can be adjusted to existing config/module naming, but the ownership boundary must remain:

- input: immutable `MaterializedFeatureSet`
- input: existing config reference only
- input: deadline/timeout context as a primitive
- output: local V3 shadow decision struct
- no runtime/session/event/execution/log writer dependencies

Define local launcher-only output types:

- `V3ShadowDecision`
- `V3ShadowVerdict`
- optional compact diagnostics/reason-chain type

V3 evaluator must:

- be deterministic for the same snapshot/config/deadline input
- classify hard risk before opportunity
- distinguish missing/degraded evidence from clean evidence
- treat execution as not-run/unavailable in P0
- never compute raw tx or live execution data
- never call active `GatekeeperBuffer::evaluate_from_features()`
- never change active policy verdicts

### Tests

Add focused tests:

- Hard manipulation/risk contradiction wins over organic opportunity.
- Missing critical evidence produces V3 shadow pending/insufficient evidence, not clean BUY.
- Deadline elapsed maps to V3 shadow timeout/timeout-evidence outcome.
- Strong organic broadening with no hard contradictions can produce V3 shadow buy candidate.
- Evaluator is deterministic for repeated calls on the same snapshot.
- Evaluator does not require `GatekeeperBuffer` or mutable session state.

Suggested command:

```bash
cargo test -p ghost-launcher gatekeeper_v3
```

### Acceptance Criteria

- New evaluator compiles as shadow-only launcher component.
- No active Gatekeeper policy file semantics change.
- No execution modules touched.
- No raw tx computation moved into `gatekeeper_v3.rs`.
- V3 Pending remains a shadow verdict only.

---

## 8. Commit 5: Runtime/Log Integration Only

### Files

- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/components/gatekeeper_v3.rs` if minor mapping helpers are needed
- Existing runtime/logging tests, likely:
  - `ghost-launcher/tests/gatekeeper_v25_regression.rs`
  - runtime tests covering terminal verdict logging
  - focused new V3 log enrichment test if no existing test fits

### Exact Intended Changes

Add a log-enrichment helper near existing buy-log enrichment helpers:

```text
enrich_buy_log_with_v3_shadow(
    buy_log: &mut GatekeeperBuyLog,
    assessment: &GatekeeperAssessment,
    config: &GatekeeperV2Config,
    deadline_elapsed: bool,
)
```

The helper must:

- read only `assessment.feature_snapshot`
- call the pure V3 evaluator from commit 4
- write only `v3_shadow_*` fields into `GatekeeperBuyLog`
- never change active verdict, active reason code, active IWIM fields, execution routing, or decision plane
- preserve existing log enrichment order unless there is a concrete reason to place V3 before/after a specific helper

Hook this helper only at `assessment.to_buy_log(...)` sites:

- active reject path: `deadline_elapsed = false`
- timeout path: `deadline_elapsed = true`
- IWIM reject path after BUY was vetoed: `deadline_elapsed = false`
- BUY path: `deadline_elapsed = false`

Do not hook V3 into `evaluate_feature_driven_terminal_verdict()` for P0.

Do not re-materialize features in `oracle_runtime.rs`.

### Tests

Add/extend tests:

- Existing active verdict remains identical with V3 log enrichment enabled.
- Active BUY/REJECT/TIMEOUT fields remain unchanged.
- Log row contains `v3_shadow_verdict` and reason diagnostics.
- IWIM reject keeps IWIM active reason/veto fields while V3 sidecar fields are present.
- Timeout row passes deadline context to V3 shadow evaluator.
- V3 shadow BUY candidate does not cause active buy routing.

Suggested commands:

```bash
cargo test -p ghost-launcher --test gatekeeper_v25_regression v3
cargo test -p ghost-launcher gatekeeper_v3
```

### Acceptance Criteria

- `GatekeeperBuffer::evaluate_from_features()` unchanged.
- Active terminal verdicts unchanged.
- IWIM policy unchanged.
- No execution path touched.
- V3 output appears only in logs as sidecar evidence.
- No separate `v3_shadow` logger plane in P0.

---

## 9. Commit 6: V3 Shadow Report Script

### Files

- New file: `scripts/v3_shadow_report.py`
- New file: `scripts/test_v3_shadow_report.py`
- Optionally reference existing patterns from:
  - `scripts/shadow_run_report.py`
  - `scripts/test_shadow_run_report.py`

### Exact Intended Changes

Create offline report script that reads current decision JSONL output and summarizes V3 shadow evidence.

The script should:

- resolve log paths using existing config conventions where possible
- read current decision JSONL rows
- prefer `v25_shadow` rows where duplicated plane rows exist
- fall back to `legacy_live` rows if no `v25_shadow` row is available
- deduplicate by `ab_record_id` if present
- otherwise deduplicate by a stable tuple such as `(pool_id, join_key, observation_start_ts_ms)`
- treat rows without V3 fields as `status=no_v3_fields`, not script failure
- never treat unknown execution status as success
- output human-readable and JSON modes if consistent with existing reporting scripts

Report sections:

- row counts and deduplication counts
- active verdict vs V3 shadow verdict matrix
- V3 reason-code distribution
- V3 evidence status distribution
- confidence buckets
- manipulation contradiction distribution
- organic broadening distribution
- missing/degraded evidence summary

### Tests

Add unit tests for:

- no V3 fields: script returns `no_v3_fields` status and exits cleanly
- mixed `legacy_live`/`v25_shadow`: no double-counting
- V3 verdict matrix aggregation
- confidence bucket aggregation
- evidence distribution aggregation
- no-dispatch/no-execution statuses are not counted as execution success

Suggested commands:

```bash
python3 -m unittest scripts/test_v3_shadow_report.py
python3 scripts/v3_shadow_report.py --config configs/rollout/shadow-burnin.toml --json
```

### Acceptance Criteria

- Reporting is offline-only.
- Missing V3 fields do not fail the report.
- Duplicated logger plane rows do not inflate counts.
- Unknown/no-dispatch/no-execution is not success.
- Script does not require runtime code changes.

---

## 10. Workspace Test Plan

Run narrow checks after each commit, then broader checks after commit 6.

Core targeted checks:

```bash
cargo test -p ghost-core feature_builder
cargo test -p ghost-core materialized
cargo test -p ghost-brain reason_code
cargo test -p ghost-brain decision_logger
cargo test -p ghost-launcher gatekeeper_v3
cargo test -p ghost-launcher --test gatekeeper_v25_regression v3
python3 -m unittest scripts/test_shadow_run_report.py
python3 -m unittest scripts/test_v3_shadow_report.py
python3 scripts/v3_shadow_report.py --config configs/rollout/shadow-burnin.toml --json
```

Final optional checks if time/resources allow:

```bash
cargo test --workspace
cargo fmt --check
```

If full workspace tests are too slow or blocked by unrelated failures, record:

- exact command run
- exact failing package/test
- whether failure is related to V3 changes
- narrow passing checks that cover the touched contracts

---

## 11. Rollback Plan

Rollback should be possible commit-by-commit.

Rollback order:

1. Revert commit 6 first if reporting breaks.
2. Revert commit 5 if runtime log enrichment causes logger/runtime issues.
3. Revert commit 4 if evaluator behavior is wrong.
4. Revert commit 3 only if schema/reason-code change is unacceptable; because it changes serialized logs, this should be treated carefully.
5. Revert commit 2 if materialization fields are wrong.
6. Revert commit 1 last, because later commits depend on the type model.

Operational rollback guardrails:

- Since P0 is shadow/log-only, disabling runtime enrichment should be enough to stop producing new V3 fields.
- Existing active decisions should remain unaffected by rollback of commits 4-6.
- If schema commit lands and logs are already emitted, readers must tolerate both rows with and without V3 fields.
- Do not delete historical JSONL logs as part of rollback.
- Do not downgrade reason-code parsing unless old and new rows remain readable.

---

## 12. Risks And Guardrails

Main risks:

- Accidental active policy change through `GatekeeperBuffer::evaluate_from_features()`.
- Dual-authority feature computation if V3 reads runtime/session state outside materialization.
- Treating missing/degraded evidence as clean.
- Treating V3 shadow Pending as active PendingCurve.
- Accidentally routing V3 shadow BUY into active buy outputs.
- Breaking old log deserialization through non-additive schema changes.
- Double-counting logger plane-expanded rows in reports.
- Introducing forbidden dependency direction `ghost-brain -> ghost-launcher`.

Guardrails:

- V3 evaluator consumes only `MaterializedFeatureSet`.
- V3 materialization happens only in `PoolObservationSession::materialize_features()`.
- V3 writes only `v3_shadow_*` fields in existing `GatekeeperBuyLog` rows.
- No separate `v3_shadow` plane in P0.
- No active verdict mutation.
- No IWIM policy mutation.
- No execution path mutation.
- No DirectBuyBuilder, DirectSellBuilder, LiveTxSender, Helius Sender, blockhash, retry, or confirmation changes.
- No HyperPrediction, Chaos, `score_pool()`, or `PoolScored` revival.
- No global `EvidenceStatus::is_actionable()` policy shortcut.
- All new serialized fields are backward compatible.

---

## 13. Final Readiness Checklist Before Implementation

Before starting commit 1:

- Confirm this final plan is accepted as the implementation contract.
- Confirm P0 remains shadow/evidence-plane only.
- Confirm no separate `v3_shadow` routed logger plane in P0.
- Confirm new V3 log fields are sidecar fields on existing decision rows.
- Confirm active Gatekeeper V2/V2.5 policy semantics remain frozen.

Before merging all commits:

- `MaterializedFeatureSet` remains SSOT.
- `PoolObservationSession::materialize_features()` remains the only V3 materialization boundary.
- `GatekeeperBuffer::evaluate_from_features()` active semantics unchanged.
- Active BUY/REJECT/TIMEOUT behavior unchanged.
- IWIM behavior unchanged.
- Execution path untouched.
- V3 shadow evaluator is deterministic and pure.
- Decision logs contain V3 diagnostics without changing active reason/verdict fields.
- Reports deduplicate logger plane-expanded rows.
- Missing/degraded evidence is visible and never clean by default.
- Rollback can remove V3 runtime enrichment without disturbing active policy.

---

## Delegation Trace

```yaml
delegation_trace:
  task_classification: "cross-cutting planning document for Ghost Decision Stack V3 P0"
  routing_performed: true
  primary_specialist: "Ghost Runtime Coordinator"
  supporting_specialists_considered:
    - "SSOT Feature Materialization Guardian"
    - "Gatekeeper Policy Auditor"
    - "Decision Logging Replay Analyst"
    - "Config Rollout Safety Reviewer"
    - "Oracle Session Runtime Engineer"
  specialist_docs_loaded:
    - "docs/agents/ghost-runtime-coordinator.md"
    - "docs/agents/ssot-feature-materialization-guardian.md"
    - "docs/agents/gatekeeper-policy-auditor.md"
    - "docs/agents/decision-logging-replay-analyst.md"
  specialist_docs_not_loaded:
    - name: "solana-execution-path-engineer.md"
      reason: "P0 explicitly forbids transaction construction, sender, blockhash, retry, and confirmation changes."
    - name: "seer-ingest-event-integrity-specialist.md"
      reason: "No parser, Yellowstone, ingest identity, or event ordering changes are planned."
    - name: "oracle-session-runtime-engineer.md"
      reason: "Runtime hook is log-enrichment only and does not alter session lifecycle; load only if implementation reveals lifecycle coupling."
    - name: "config-rollout-safety-reviewer.md"
      reason: "No config fields are planned for P0; use if implementation introduces thresholds or rollout toggles."
  skills_used:
    - "ghost-execution"
    - "abstract-reasoning"
  fast_path_used: false
  contracts_checked:
    - "MaterializedFeatureSet SSOT"
    - "PoolObservationSession::materialize_features materialization boundary"
    - "GatekeeperBuffer::evaluate_from_features active policy boundary"
    - "typed GatekeeperReasonCode taxonomy"
    - "DecisionLogger schema compatibility"
    - "shadow/live separation"
    - "ghost-launcher -> ghost-brain dependency direction"
    - "active-vs-legacy discipline"
    - "no execution-path changes"
  unresolved_routing_uncertainty:
    - "Exact V3 threshold names and values must be confirmed during implementation if the existing config lacks suitable fields."
```
