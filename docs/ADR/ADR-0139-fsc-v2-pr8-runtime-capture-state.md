# ADR-0139: FSC v2 PR8 runtime capture state

**Date:** 2026-05-31
**Status:** Accepted as current PR8 operational state
**Author:** Codex, based on FSC v2 NLN implementation work
**Follows:** `ADR-0138-fsc-v2-nln-program-streams-capture-evidence.md`
**Plan:** `PLANS/PLAN_FSC_V2_NLN_CAPTURE_EVIDENCE_20260529.md`

## Context

`ADR-0138` allowed FSC v2 to move from dormant infrastructure into an additive capture/evidence
lane powered by NLN Program Streams. That decision explicitly did not activate FSC as a Gatekeeper
policy signal, penalty, hard reject, size-down rule or promotion-readiness requirement.

The implementation has now reached the PR8 runtime-capture stage. The repository contains the
pieces required to run a long FSC v2 capture and qualification burn-in:

- NLN Program Streams client and event normalization for `pumpfun.create`, `pumpfun.trade` and
  `system.transfers`;
- native-SOL-only transfer normalization for the primary FSC lane;
- additive `tx_index` propagation where provider data can supply transaction ordering;
- FSC v2 dominant-source attribution, strict pre-buy ordering rules and sample-normalized HHI
  evidence;
- additive FSC v2 materialization/logging fields that do not overwrite legacy
  `funding_source_concentration`;
- provider qualification tooling that writes selector datasets and reports from captured NLN
  artifacts;
- a dedicated PR8 capture rollout profile and artifact-builder loop.

The current objective is a minimum 24h run that proves capture stability, artifact durability,
coverage shape and report behavior. It is not a decision-system promotion run.

## Current Runtime State

The PR8 capture profile is:

```text
configs/rollout/shadow-burnin-v3-fsc-capture-nln-r1.toml
```

The active PR8 run uses:

```text
target/release/ghost-launcher --config configs/rollout/shadow-burnin-v3-fsc-capture-nln-r1.toml
```

and a sidecar artifact loop:

```text
scripts/run_fsc_v2_pr8_artifact_builder_loop.sh shadow-burnin-v3-fsc-capture-nln-r1
```

The intended durable outputs are:

```text
logs/nln_capture/<scope>/pumpfun_create_raw_v1.jsonl
logs/nln_capture/<scope>/pumpfun_trade_raw_v1.jsonl
logs/nln_capture/<scope>/system_transfers_raw_v1.jsonl
datasets/selector/<scope>/nln_candidate_birth_v1.jsonl
datasets/selector/<scope>/funding_events_v1.jsonl
datasets/selector/<scope>/fsc_snapshots_v2.jsonl
reports/selector/<scope>/fsc_coverage_v2.json
reports/selector/<scope>/nln_provider_benchmark_v1.json
reports/selector/<scope>/decision_time_vs_eventual_fsc_v1.json
reports/selector/<scope>/fsc_provider_qualification_manifest_v1.json
```

The scope used for the PR8 run is:

```text
shadow-burnin-v3-fsc-capture-nln-r1
```

## What We Have

The current repository state provides the following FSC v2 behavior.

1. NLN Program Streams are treated as a semantic event layer, not as R2 canonical truth.
2. The Seer NLN path subscribes to the configured Program Streams topics and captures normalized
   raw artifacts.
3. NLN `system.transfers` events can feed the existing Ghost funding transfer boundary for native
   SOL only.
4. The funding lane is still isolated from active BUY/REJECT decisions.
5. FSC v2 attribution uses dominant meaningful source selection instead of latest-transfer-wins.
6. Same-slot cross-signature ordering requires deterministic ordering evidence instead of arrival
   timestamp proof.
7. FSC v2 evidence includes coverage, health, unknown, neutral, low-confidence and ordering
   diagnostics.
8. FSC v2 primary score is sample-normalized HHI over non-neutral known buyer funding sources.
9. Legacy `funding_source_concentration` is not silently redefined as FSC v2.
10. Decision logs can carry FSC v2 evidence for offline review.
11. Provider qualification tooling is fail-closed for missing audit data, incomplete keys and
    insufficient duration.
12. Numeric JSON strings from provider artifacts are handled by the qualification tooling.
13. Raw official artifact paths are preserved without duplicating bytes when hardlinks are possible.
14. The capture profile stores only environment variable names for secrets, not API key values.

## Current Problems and Known Gaps

The current run is expected to report incomplete qualification until the required evidence exists.
The following limitations are known and accepted for PR8 capture, but block policy promotion:

1. **Minimum duration gate:** provider qualification is not complete until at least 24h of capture
   exists. A 72h run remains preferred before stronger conclusions.
2. **Audit feed gap:** `nln_provider_benchmark_v1.json` remains `NO-GO` when no Chainstack/raw
   Yellowstone/archive-capable audit event source is provided.
3. **Decision-time vs eventual gap:** `decision_time_vs_eventual_fsc_v1.json` remains incomplete
   until eventual/postfill reconstruction exists for comparison.
4. **Coverage is empirical, not assumed:** early unknown rate, neutral share and known
   non-neutral coverage must be measured from the run instead of inferred from provider claims.
5. **Create/trade coverage still needs validation:** `pumpfun.create` and `pumpfun.trade`
   artifacts are useful candidate-universe evidence, but not yet canonical proof for all
   denominator work.
6. **Program Streams are not R2 SSOT:** R2 canonical market path still requires raw Yellowstone
   AccountUpdates, DIAG or canonical account-state snapshots.
7. **Provider offsets are diagnostic:** offset continuity is not treated as authoritative resume
   proof without provider-documented semantics.
8. **Reconnect without resume degrades evidence:** a reconnect gap inside the active lookback
   window cannot produce clean FSC evidence.
9. **Disk pressure is real:** high-volume transfer artifacts require bounded retention behavior.
   PR8 uses deterministic artifact sampling for raw transfer files while the runtime FSC index can
   still observe the live transfer stream.
10. **No active scoring evidence yet:** baseline and ablation reports have not proven that FSC v2
    improves R1/R2 outcomes.

## Decision

The current PR8 state is accepted as a capture/evidence runtime, under these constraints:

1. Continue the 24h capture run using the dedicated FSC v2 NLN capture profile.
2. Persist PR8 source code, config, scripts, provider specification and this ADR.
3. Do not commit generated `logs/`, `datasets/` or `reports/` artifacts as source state.
4. Keep `fsc_v2.decision_enabled=false`.
5. Keep `fsc_v2.hard_reject_enabled=false`.
6. Keep active Gatekeeper policy behavior unchanged.
7. Keep Program Streams outside the R2 SSOT contract.
8. Treat all PR8 reports as qualification evidence, not as promotion approval.
9. Treat missing audit source, insufficient duration and incomplete event keys as fail-closed report
   states.
10. Use environment variables or external secret management for the NLN API key; do not store the
    key in repository files.

## Verification State

The implementation state was validated with the following checks before this ADR:

```text
cargo fmt -p ghost-launcher -- --check
cargo check -p ghost-launcher
cargo test -p ghost-launcher config::tests::test_fsc_v2_capture_profile_loads_capture_only_program_streams --lib
cargo test -p ghost-launcher config::tests::test_seer_program_streams_config_surface_deserializes --lib
cargo build --release -p ghost-launcher
python3 -m py_compile scripts/build_fsc_v2_provider_qualification.py scripts/test_fsc_v2_provider_qualification.py
python3 -m unittest scripts.test_fsc_v2_provider_qualification
bash -n scripts/run_fsc_v2_pr8_artifact_builder_loop.sh
git diff --check
```

Runtime proof is still in progress. Passing compile and unit checks does not prove provider
coverage, 24h stability, audit overlap or FSC predictive value.

## Non-Goals

This ADR does not authorize:

- active FSC penalty;
- active FSC hard reject;
- active FSC size-down;
- active FSC combo veto;
- using FSC v2 as promotion-readiness evidence;
- treating `UNKNOWN` funding as clean `0.0`;
- treating neutral hubs as cabal sources;
- treating Program Streams as R2 canonical market path;
- removing the raw Yellowstone/DIAG/snapshot R2 requirement;
- storing provider API keys in the repository;
- committing generated run artifacts as source code.

## Next Gates

FSC v2 cannot move beyond capture/evidence until all of the following are complete:

1. Minimum 24h PR8 run completes without unacceptable stream stalls, queue drops or process
   instability.
2. Provider benchmark includes a valid external audit source.
3. `system.transfers`, `pumpfun.trade` and `pumpfun.create` coverage are measured and reported.
4. Decision-time FSC and eventual FSC are compared.
5. Same-slot unorderable rate is quantified.
6. Unknown, neutral and low-confidence rates are quantified.
7. Baseline offline ablation proves `baseline_core + FSC v2` improves holdout R1/R2 outcomes
   without dishonest denominator shrinkage.
8. Shadow counterfactual policy impact is measured.
9. A separate ADR explicitly authorizes any future `fsc_v2.decision_enabled=true` mode.

Until then, the correct operational mode is:

```text
FSC_CAPTURE = ON
FSC_IN_DATASET = ON
FSC_IN_SCORING_OFFLINE = pending validation
FSC_IN_ACTIVE_GATEKEEPER = OFF
FSC_AS_HARD_REJECT = OFF
```
