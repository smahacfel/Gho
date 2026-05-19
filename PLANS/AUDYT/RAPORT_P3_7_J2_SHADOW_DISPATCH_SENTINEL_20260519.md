# P3.7-J2 Shadow Dispatch Sentinel Validation

Date: 2026-05-19

Status: **INCONCLUSIVE**

Decision:

- Full R14 collection remains **HOLD**.
- P3.7-J1 join-key propagation is still **not runtime-validated end-to-end** through shadow dispatch artifacts.
- V3/MFS replay path is **PASS** for this sentinel namespace.
- Shadow dispatch path is **INCONCLUSIVE** because no Gatekeeper BUY occurred during the sentinel budget.
- No P2, live, runtime threshold tuning, IWIM change, live sender change, or policy mutation was performed.

## Scope

Namespace:

`shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel`

Config:

`configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel.toml`

Goal:

Validate that a real shadow BUY/dispatch after P3.7-J1 carries stable join metadata from decision artifacts into shadow transport, shadow entry, and lifecycle artifacts.

## Preflight

Direct runtime preflight:

- command: `cargo run --quiet -p ghost-launcher --bin ghost-launcher -- --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel.toml --preflight`
- result: **PASS**
- relevant checks: shadow-only entry mode, shadow execution mode, writable shadow/event/decision directories, trigger keypair, RPC version, Seer gRPC endpoint, metrics port.

Formal wrapper preflight:

- command: `bash scripts/ghost_production_preflight.sh --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel.toml`
- result: **BLOCKED**
- reason: stale `.ghost/baseline_accepted_revision`
- expected HEAD: `c8d2ea5f78555c0c42d187938d47dcdd3d047577`
- current stamp: `256efc4419bf900c223016d44e1fc73f37471ac4`

No baseline stamp was updated during J2.

## Runtime

Runtime command:

```bash
timeout 4h env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel.toml
```

Runtime was stopped after reaching the lower sentinel budget of approximately 500 V3/V2.5 rows.

- elapsed runtime: approximately `01:10:28`
- process exit: controlled termination after sentinel budget
- observed shell/session exit code: `143`
- stop reason: `max_v3_rows_lower_bound_reached_without_shadow_buy`

## Decision-Side Evidence

Post-run V3 shadow report:

`logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel/v3_shadow_report_after_j2.json`

Key results:

- status: `ok`
- raw_rows: `505`
- deduped_rows: `505`
- v3_rows: `505`
- bad_rows: `0`
- no_v3_rows: `0`
- replay.status: `full`
- full_snapshot_payload_rows: `505`
- hash_only_rows: `0`
- stale_against_config: `false`
- v3_policy_config_hash coverage: `505 / 505`
- v3_feature_snapshot_hash coverage: `505 / 505`
- execution outcomes: `missing=505`, success_count=`0`

Post-run strict full replay report:

`logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel/v3_full_replay_report_after_j2.json`

Key results:

- status: `ok`
- replay_status: `full_replay_ok`
- total_rows: `505`
- v3_rows: `505`
- bad_rows: `0`
- status_counts.full_replay_ok: `505`

Decision verdict distribution at stop:

- v2.5/v25_shadow rows: `505`
- v2.5 decision_verdict_buy: `0`
- v2.5 verdict_type counts:
  - `REJECT_PDD_ENTRY_DRIFT`: `278`
  - `REJECT_PDD_WHALE`: `179`
  - `REJECT_PDD_FLASH_CRASH`: `26`
  - `REJECT_PDD_SPIKE`: `12`
  - `REJECT_LOW_TRAJECTORY`: `4`
  - `REJECT_PDD_RAMPING`: `3`
  - `HARD_FAIL_MARKET_CAP`: `1`
- v3_shadow_verdict counts:
  - `REJECT`: `362`
  - `PENDING`: `141`

Seer runtime coverage rows at stop:

- `logs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel/decisions/seer_runtime_coverage_audit.jsonl`: `2367`

## Shadow-Side Evidence

No shadow BUY/dispatch occurred during the sentinel budget.

Shadow artifact status:

- `logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel/buys.jsonl`: missing / `0` rows
- `logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel/shadow_entries.jsonl`: missing / `0` rows
- `logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel/shadow_lifecycle.jsonl`: missing / `0` rows
- shadow-onchain lifecycle report: not run; no lifecycle rows exist
- shadow lifecycle labeler: not run; no lifecycle rows exist
- feature availability over lifecycle labels: not run; no lifecycle rows exist

This is not classified as a join-key propagation failure because no row traversed the shadow transport/entry/lifecycle path.

## Join-Key Audit

Post-run join-key audit:

`PLANS/AUDYT/RAPORT_P3_7_J2_SENTINEL_JOIN_KEY_AUDIT_20260519.md`

JSON:

`logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel/p3_7_mfs_lifecycle_join_key_audit_after_j2.json`

Key results:

- readiness: `not_ready`
- readiness reasons:
  - `missing_shadow_transport_rows`
  - `missing_shadow_entry_rows`
  - `missing_shadow_lifecycle_rows`
- decision_rows: `2880`
- v3_payload_rows: `2880`
- decision_rows_with_ab_record_id: `2880`
- shadow_transport_rows: `0`
- shadow_entry_rows: `0`
- shadow_lifecycle_rows: `0`
- shadow_transport_rows_with_ab_record_id: `0`
- shadow_entry_rows_with_ab_record_id: `0`
- shadow_lifecycle_rows_with_ab_record_id: `0`

The audit reports `join_quality=exact_ab_record_id` for the decision artifacts that exist, but full-chain join-key coverage is not established because there are no shadow artifacts.

## Classification

J2 result: **INCONCLUSIVE**

Reason:

`no_shadow_buy_no_dispatch_within_sentinel_budget`

What was proven:

- The J2 profile starts under direct runtime preflight.
- V3/MFS replay payload rows are emitted in the J2 namespace.
- V3 replay is strict-clean for 505 rows.
- Decision-side `ab_record_id`, V3 feature snapshot hash, and V3 policy config hash coverage are complete.
- No live/P2/promotion/tuning path was activated.

What was not proven:

- Decision -> shadow transport join-key propagation.
- Shadow transport -> shadow entry join-key propagation.
- Shadow entry -> shadow lifecycle join-key propagation.
- Shadow-onchain lifecycle report over forward V3/MFS rows.
- Feature availability over lifecycle-labeled V3/MFS rows.

## Decision

Full R14 collection remains **HOLD**.

P3.7-J1 additive join-key repair remains code/test validated but not runtime-validated end-to-end through a real shadow BUY dispatch.

Next operational decision:

- Do not run full R14 yet.
- Either run a longer J2 sentinel under the same no-tuning policy, or design a separate documented test/integration harness that exercises the shadow BUY path with join metadata without changing active runtime policy.

## Non-Goals Preserved

- No P2.
- No live.
- No active V2/V2.5 policy change.
- No V3 selector prototype.
- No runtime threshold tuning.
- No IWIM change.
- No live sender change.
- No MFS extension as policy.
- No lifecycle outcome was treated as a decision-time feature.
