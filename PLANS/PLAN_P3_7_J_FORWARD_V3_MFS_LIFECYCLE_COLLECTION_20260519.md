# P3.7-J Forward V3/MFS + Shadow-Burnin Lifecycle Collection

Status: execution-ready design.

This is a collection run, not a selector candidate.

## 1. Goal

Collect a forward-only dataset that contains, in the same isolated namespace:

- V3 `MaterializedFeatureSet` / replay payload evidence,
- V3 shadow decision telemetry,
- Gatekeeper V2/V2.5 decision context,
- shadow-burnin transport, entry, and lifecycle artifacts,
- shadow/on-chain lifecycle truth,
- shadow lifecycle labels,
- feature availability and join-key coverage reports.

The purpose is to build the first P3.7 dataset where V3/MFS decision-time
features can be joined to shadow lifecycle/on-chain truth.

## 2. Source Decision

P3.7-I found:

- `joined_feature_rows = 738`
- primary Gatekeeper-context subset: `154` dirty-good vs `417` bad
- `signal_level = moderate_diagnostic_signal`
- `v3_selector_prototype_allowed = false`
- recommendation:
  `design_forward_v3_mfs_lifecycle_collection_run`

That is enough to design a new collection run. It is not enough to start a V3
selector prototype on the recovered historical dataset.

## 3. Non-Goals

This plan does not authorize:

- P2,
- live execution,
- runtime threshold tuning,
- active V2/V2.5 policy changes,
- IWIM changes,
- live sender changes,
- MFS schema extension as part of this task,
- FSC active gate,
- treating shadow simulation as live inclusion,
- treating lifecycle labels as decision-time features,
- treating speculative finality as finalized proof.

## 4. New Artefacts

Design/config artefacts:

- `docs/ADR/ADR-0135-v3-p37-forward-mfs-lifecycle-collection.md`
- `configs/rollout/ghost_brain_v3_p37_mfs_lifecycle.toml`
- `configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml`
- `configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only.toml`
- `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`

Expected generated artefacts after smoke/collection:

- `logs/rollout/<namespace>/decisions/**/gatekeeper_v2_decisions.jsonl`
- `logs/rollout/<namespace>/decisions/**/gatekeeper_v2_buys.jsonl`
- `logs/shadow_run/<namespace>/buys.jsonl`
- `logs/shadow_run/<namespace>/shadow_entries.jsonl`
- `logs/shadow_run/<namespace>/shadow_lifecycle.jsonl`
- `datasets/events/<namespace>/**`
- `logs/shadow_run/<namespace>/shadow_onchain_lifecycle_report.jsonl`
- `logs/shadow_run/<namespace>/p3_7_shadow_lifecycle_labels.jsonl`
- `logs/shadow_run/<namespace>/p3_7_shadow_lifecycle_feature_availability.json`

## 5. Config Contract

Brain config:

```text
configs/rollout/ghost_brain_v3_p37_mfs_lifecycle.toml
```

Required invariants:

```text
gatekeeper_v3.enabled = false
gatekeeper_v3.shadow_emit_enabled = true
gatekeeper_v3.replay_payload_enabled = true
gatekeeper_v3.promotion.enabled = false
gatekeeper_v3.evidence_requirements.fsc = false
gatekeeper_v3.evidence_requirements.execution = false
```

Launcher profiles:

```text
configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml
configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only.toml
```

Required invariants:

```text
mode = "production"
seer.stream_mode = "single_global"
seer.tx_filter_strategy = "per_pool"
seer.funding_lane_mode = "disabled"
trigger.entry_mode = "shadow_only"
trigger.shadow_run.enabled = true
trigger.shadow_run.emit_event_bus = true
trigger.shadow_run.payer_strategy = "ephemeral"
execution.execution_mode = "shadow"
execution.shadow.timing_model = "prepared_entry_mirror"
execution.shadow.stale_policy = "emit_warning"
```

## 6. Namespace Contract

Smoke namespace:

```text
shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke
```

Primary collection namespace:

```text
shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only
```

All logs, event datasets, WAL/snapshot dirs, decision logs, shadow transport,
entry logs, and lifecycle logs must stay inside the namespace. Do not reuse or
mutate historical R10/R11/R13/P3.6 configs or artifacts.

## 7. Join-Key Contract

The run must make join quality measurable across decision, V3 payload, shadow
transport, shadow entry, shadow lifecycle, and reports.

Required or preferred fields:

```text
ab_record_id
candidate_id
position_id
pool_id
base_mint / mint_id
decision_ts_ms
observation_start_ts_ms
observation_end_ts_ms
v3_feature_snapshot_hash
v3_policy_config_hash / config_hash
decision_plane
rollout_namespace / rollout_profile
```

The new audit script:

```bash
python3 scripts/v3_p37_mfs_lifecycle_join_key_audit.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only.toml \
  --output-json logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only/p3_7_mfs_lifecycle_join_key_audit.json \
  --output-md PLANS/AUDYT/RAPORT_P3_7_R14_MFS_LIFECYCLE_JOIN_KEY_AUDIT_202605XX.md
```

Acceptance target: joins should be candidate/AB-level wherever possible, not
mostly `pool_id + mint + time window`.

## 8. Smoke Run

Run smoke before the primary collection.

Command shape:

```bash
cargo test --workspace --no-run

bash scripts/ghost_production_preflight.sh \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml

timeout 30m env RUST_LOG=info \
  cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml
```

Smoke acceptance:

- launcher config loads,
- brain config loads,
- decision rows are produced,
- V3 rows are produced,
- full V3 replay payload rows equal V3 rows,
- hash-only V3 rows are zero,
- shadow transport path is writable,
- shadow entry path is writable,
- shadow lifecycle path is writable,
- no live transaction requirement appears,
- no P2/promotion/tuning is enabled.

If no BUY/shadow dispatch occurs, smoke may pass only as V3/MFS payload
readiness. Lifecycle readiness remains inconclusive.

## 9. Primary Collection Run

Run only after smoke acceptance.

Command shape:

```bash
timeout <duration> env RUST_LOG=info \
  cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only.toml
```

Initial target:

- minimum `1000` V3 rows,
- preferred `2000-3000` V3 rows,
- minimum useful lifecycle target: at least one positive and one bad lifecycle
  label,
- preferred lifecycle target: `>=100` lifecycle labels with V3/MFS feature
  payload coverage.

Do not loosen policy thresholds only to force BUY/lifecycle rows. If BUY rate is
low, report low BUY/lifecycle collection as a data-collection limitation.

## 10. Post-Run Pipeline

Run V3 report:

```bash
python3 scripts/v3_shadow_report.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only.toml \
  --json
```

Run strict replay:

```bash
python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only.toml \
  --strict \
  --json
```

Run shadow/on-chain lifecycle report:

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only.toml \
  --all-sessions \
  --output logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only/shadow_onchain_lifecycle_report.jsonl
```

Run lifecycle labeler:

```bash
python3 scripts/v3_p37_shadow_lifecycle_labeler.py \
  --shadow-onchain-lifecycle logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only/shadow_onchain_lifecycle_report.jsonl \
  --output logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only/p3_7_shadow_lifecycle_labels.jsonl \
  --summary-output logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only/p3_7_shadow_lifecycle_label_summary.json \
  --summary-md-output PLANS/AUDYT/RAPORT_P3_7_R14_SHADOW_LIFECYCLE_LABELS_202605XX.md
```

Run feature availability:

```bash
python3 scripts/v3_p37_shadow_lifecycle_feature_availability.py \
  --shadow-lifecycle-labels logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only/p3_7_shadow_lifecycle_labels.jsonl \
  --shadow-onchain-lifecycle logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only/shadow_onchain_lifecycle_report.jsonl \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only.toml \
  --output-json logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only/p3_7_shadow_lifecycle_feature_availability.json \
  --output-md PLANS/AUDYT/RAPORT_P3_7_R14_MFS_LIFECYCLE_FEATURE_AVAILABILITY_202605XX.md
```

Run join-key audit:

```bash
python3 scripts/v3_p37_mfs_lifecycle_join_key_audit.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only.toml \
  --output-json logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only/p3_7_mfs_lifecycle_join_key_audit.json \
  --output-md PLANS/AUDYT/RAPORT_P3_7_R14_MFS_LIFECYCLE_JOIN_KEY_AUDIT_202605XX.md
```

## 11. Gate After Collection

Diagnostic V3/MFS lifecycle feature prototype can start only if:

- strict V3 replay passes,
- V3/MFS replay payload coverage is nonzero,
- shadow lifecycle labels exist,
- at least one positive and one bad lifecycle label exist,
- feature availability confirms V3/MFS coverage,
- join-key audit shows candidate/AB-level coverage is usable,
- shadow/on-chain lifecycle truth has resolved rows.

If not, report the blocker explicitly:

- no BUY/lifecycle rows,
- no V3/MFS coverage,
- join-key mismatch,
- unresolved truth,
- only speculative/dirty labels,
- insufficient class balance.

## 12. Use Of P3.7-I Hints

P3.7-I diagnostic hints are design inputs only, not runtime thresholds.

Fields to monitor in the forward V3/MFS dataset:

- `flipper_presence_ratio`
- `max_tx_per_signer_observed`
- `max_single_tx_price_impact_pct_observed`
- `entry_drift_pct` / `pdd_entry_drift_pct`
- `dev_has_sold`
- `dev_sold_within_3s`
- `v25_shadow_observation_stage`

Several directions are likely contextual or momentum-correlated. Do not encode
them as rules without forward V3/MFS+lifecycle validation.

## 13. Test Plan

Static Python checks:

```bash
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
```

Brain config load:

```bash
cargo test -p ghost-brain --test ghost_brain_config_load_test gatekeeper_v3_p37_mfs_lifecycle_collection_descopes_fsc_forward_only -- --nocapture
```

Launcher profile load:

```bash
cargo test -p ghost-launcher config::tests::test_p37_mfs_lifecycle_rollout_profiles_load_shadow_only_primary_only -- --nocapture
```

Formatting:

```bash
git diff --check
```

## 14. Expected State

After this design stage:

- R14 smoke and primary-only configs exist.
- Brain V3/MFS lifecycle config exists.
- ADR-0135 formalizes the collection decision.
- Join-key coverage is measurable.
- No historical config is mutated.
- No P2/live/tuning/promotion is enabled.
