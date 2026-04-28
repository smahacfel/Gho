# ADR-0103: FSC unlock implementation audit against PR-1..PR-4 plan

**Date:** 2026-04-17
**Status:** Accepted
**Author:** Ghost Father

## Context

User requested a second-pair-of-eyes audit of the repository after the planned FSC unlock rollout was reported as implemented end-to-end.

The acceptance baseline for this audit was:

- `/root/Gho/PLANS/FSC_UNLOCK.md`
- `/root/Gho/docs/ADR/ADR-0102-authoritative-funding-lane-unlock-plan.md`

The audit had to verify, without changing SSOT or runtime semantics, whether all four planned PR stages were actually present in code and whether their merge criteria were satisfied.

## Decision

The repository now contains the planned FSC unlock implementation across all four rollout stages.

### PR-1 — Contract freeze + additive funding provenance contract

**Accepted as implemented.**

Evidence found:

- `off-chain/components/seer/src/ipc.rs`
  - additive provenance contract exists via `FundingTransferProvenance`
  - stable downstream bit remains `full_chain_coverage`
  - legacy/default filtered contract stays omitted from JSON when provenance is default
- `ghost-launcher/src/events.rs`
  - launcher event contract mirrors the additive provenance fields
  - legacy/default filtered serialization compatibility is preserved
- `ghost-launcher/src/components/seer.rs`
  - provenance is propagated 1:1 from Seer IPC into launcher event bus
- `grpc_global_stream` remains explicitly documented and implemented as filtered, not authoritative

Acceptance evidence:

- backward-compatible serde tests exist in both Seer IPC and launcher event surfaces
- no audited production path upgrades the legacy filtered lane to `full_chain_coverage=true`

### PR-2 — Seer authoritative funding lane (disabled by default)

**Accepted as implemented.**

Evidence found:

- `off-chain/components/seer/src/config.rs`
  - `FundingLaneMode::{Disabled, PumpFiltered, FullChain}` exists
  - default is fail-closed: `Disabled`
- `off-chain/components/seer/src/grpc_connection.rs`
  - dedicated `GrpcSubscriptionProfile::{FundingLanePumpFiltered, FundingLaneFullChain}` exists
  - `FundingLaneFullChain` uses all-transactions profile without account filters
  - `PrimaryGlobal` remains separate and unchanged in meaning
- `off-chain/components/seer/src/lib.rs`
  - explicit mapping from source label to `(full_chain_coverage, provenance)`
  - `grpc_global_stream` always maps to filtered provenance
  - authoritative truth can only come from `grpc_funding_lane_full_chain` with `FundingLaneMode::FullChain`
  - dedicated funding lane runs in its own loop and is kept separate from trade detection/buffering

Acceptance evidence:

- default config keeps authoritative lane off
- lane separation tests exist and pass
- dedicated funding lane does not hijack normal trade path semantics

### PR-3 — Launcher/runtime readiness wiring

**Accepted as implemented.**

Evidence found:

- `ghost-launcher/src/main.rs`
  - startup no longer relies on a blind hardcoded promotion to authoritative availability
  - runtime starts fail-closed using a watch channel seeded with `false`
- `ghost-launcher/src/components/seer.rs`
  - availability signal is wired only when Seer can expose the authoritative funding-lane availability sender
  - if not eligible, launcher logs that FSC availability remains fail-closed
- `off-chain/components/seer/src/grpc_connection.rs`
  - `LaneAvailabilityTracker` publishes lane health based on actual connected workers
- `ghost-launcher/src/oracle_runtime.rs`
  - runtime subscribes to the availability watch channel and updates session state live
  - readiness promotion stays separate from mere startup wiring
- `ghost-launcher/src/tx_intelligence/funding_source.rs`
  - `FundingSourceIndex` remains the sole stateful FSC source
  - warmup requires both `stream_available` and at least one authoritative transfer
  - filtered transfers do not unlock readiness
- `ghost-launcher/src/session/observation.rs`
  - canonical materialization path remains `MaterializedFeatureSet.sybil_resistance.funding_source_concentration`

Acceptance evidence:

- filtered transfer fail-closed tests exist and pass
- stream unavailable / rolling-state unavailable degraded-reason tests exist and pass
- policy path still consumes only materialized sybil features, not direct index reads

### PR-4 — Observability + bake package + rollout guardrails

**Accepted as implemented for code/docs/harness scope.**

Evidence found:

- `ghost-launcher/src/oracle_metrics.rs`
  - FSC metrics exist for availability, warmup, lookup hits/misses, hit-rate, prune duration, bounded-index pressure
- `off-chain/components/seer/src/lib.rs`
  - Seer emits `seer_funding_transfer_observations_total{lane=...,coverage=...}`
- `docs/RUNBOOK_HOT_PATH_METRICS.md`
  - authoritative funding lane metrics and interpretation are documented
- `docs/RUNBOOK_PRODUCTION_ROLLOUT.md`
  - neutral-disabled replay diff flow is documented
  - authoritative-enabled paper-burnin flow is documented
  - rollback and abort conditions are documented
- `scripts/fsc_replay_diff.py`
  - replay diff harness exists and enforces the intended neutral-disabled vs authoritative-enabled drift contracts
- committed profiles remain neutral:
  - `config.toml`
  - `configs/rollout/paper-burnin.toml`
  - `configs/rollout/dual-micro-live.toml`
  - `configs/rollout/future-live.toml`
  all keep `funding_lane_mode = "disabled"`

Acceptance evidence:

- observability surfaces exist in code
- bake checklist and rollback path exist in docs
- replay diff harness exists in repo
- committed profiles remain fail-closed by default

### Audit caveat

The repository does **not** contain committed operator-facing bake result artifacts such as:

- `gatekeeper_v2_decisions.jsonl`
- paired neutral/authoritative replay artifacts suitable for direct post-hoc diffing
- committed `shadow_run_report` output
- committed `seer_runtime_coverage_audit.jsonl`

Therefore this audit accepts the **implementation** and the **acceptance scaffolding**, but it does **not** independently prove that an operator already executed the authoritative bake workflow and archived its outputs in-repo.

## Architectural Impact

This audit confirms that the intended architecture from `ADR-0102` is now present in repository state:

1. Seer keeps filtered trade ingest and authoritative funding ingest as separate planes.
2. Launcher/runtime stays fail-closed until real authoritative lane health and authoritative transfers exist.
3. `FundingSourceIndex` remains the only stateful FSC lookup source.
4. Canonical FSC still reaches policy only through `MaterializedFeatureSet.sybil_resistance`.
5. Observability and rollout tooling are present without silently enabling FSC policy penalties.

## Risk Assessment

**Rate:** Low/Medium

- **Low** risk that PR-1/PR-2 semantics were only planned but not implemented — code and tests are present.
- **Low** risk that filtered funding observations can accidentally unlock FSC readiness on the audited path — explicit guards and tests are present.
- **Medium** risk of over-claiming rollout completion if someone interprets repo state as proof that authoritative bake was already executed; the operator evidence package is not committed in the repository.

## Consequences

What becomes easier:

- future FSC policy follow-up can build on real contract/runtime/observability scaffolding
- audits can now point to concrete tests and files instead of architectural intent only
- rollback remains config-first because committed profiles stay neutral

What remains harder:

- proving historical operator bake execution still requires external or uncommitted artifacts
- live rollout sign-off still needs environment-specific evidence, not just repository inspection

## Alternatives Considered

### 1. Declare the rollout incomplete because no operator bake artifacts are committed

Rejected.

The planned PR-1..PR-4 scope was primarily code, contract, runtime, observability and rollout tooling. Those pieces are present.

### 2. Declare the rollout fully proven end-to-end with no caveats

Rejected.

Repository state alone does not prove that an operator executed the authoritative bake workflow and archived the resulting decision/report artifacts.

### 3. Ignore targeted tests and rely only on grep-level evidence

Rejected.

Targeted tests were required to confirm that the critical fail-closed and compatibility semantics actually hold.

## Validation Steps

Repository inspection covered at least these files:

- `off-chain/components/seer/src/config.rs`
- `off-chain/components/seer/src/ipc.rs`
- `off-chain/components/seer/src/grpc_connection.rs`
- `off-chain/components/seer/src/lib.rs`
- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/main.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/tx_intelligence/funding_source.rs`
- `ghost-launcher/src/oracle_metrics.rs`
- `docs/RUNBOOK_HOT_PATH_METRICS.md`
- `docs/RUNBOOK_PRODUCTION_ROLLOUT.md`
- `scripts/fsc_replay_diff.py`

Targeted tests executed during this audit:

- `cargo test -p seer --lib tests::test_process_event_preserves_funding_lane_boundaries -- --exact -q`
- `cargo test -p seer --lib grpc_connection::tests::subscribe_pump_filtered_funding_lane_disables_account_filters -- --exact -q`
- `cargo test -p seer --lib grpc_connection::tests::subscribe_full_chain_funding_lane_uses_all_transactions_without_accounts -- --exact -q`
- `cargo test -p seer --lib tests::test_dedicated_funding_lane_skips_trade_detection_and_buffering -- --exact -q`
- `cargo test -p seer --lib ipc::tests::test_filtered_funding_transfer_serialization_omits_default_provenance -- --exact -q`
- `cargo test -p seer --lib ipc::tests::test_legacy_funding_transfer_fixture_deserializes_with_filtered_defaults -- --exact -q`
- `cargo test -p ghost-launcher --lib tx_intelligence::funding_source::tests::filtered_transfer_does_not_mark_funding_stream_available -- --exact -q`
- `cargo test -p ghost-launcher --lib tx_intelligence::funding_source::tests::stream_unavailable_returns_stream_reason -- --exact -q`
- `cargo test -p ghost-launcher --lib tx_intelligence::funding_source::tests::warmup_unavailable_returns_rolling_state_reason -- --exact -q`
- `cargo test -p ghost-launcher --lib components::seer::tests::seer_funding_transfer_emits_funding_transfer_observed -- --exact -q`
- `cargo test -p ghost-launcher --lib events::tests::funding_transfer_observed_default_filtered_serialization_omits_provenance -- --exact -q`
- `cargo test -p ghost-launcher --lib events::tests::funding_transfer_observed_legacy_fixture_deserializes_with_filtered_defaults -- --exact -q`
- `cargo test -p ghost-launcher --lib events::tests::test_funding_transfer_observed_event -- --exact -q`
- `cargo test -p ghost-launcher --lib oracle_metrics::tests::eventbus_metrics_register_and_update -- --exact -q`

Audit verdict:

- **PR-1:** accepted
- **PR-2:** accepted
- **PR-3:** accepted
- **PR-4:** accepted for implementation/tooling scope, with explicit caveat that committed operator bake outputs were not found in-repo
- **Overall:** implementation matches the four-PR unlock plan; historical bake execution is not independently proven from repository artifacts alone
