# ADR-0129: V3 full replay payload hash boundary - 2026-05-16

**Date:** 2026-05-16
**Status:** Accepted
**Author:** Ghost Father

## Context

P3.2 introduced gated V3 full replay payload emission for shadow-only audit rows:

- `v3_replay_payload_schema_version`
- `v3_materialized_feature_snapshot`
- `v3_policy_config_payload`

The intent is to move V3 replay evidence from `hash_only` to auditable full replay. The replay validator is fail-closed and must distinguish:

- `hash_only`
- `payload_absent`
- `payload_schema_unsupported`
- `payload_hash_mismatch`
- `policy_hash_mismatch`
- `full_replay_ok`

During the controlled P3.2 r2 shadow rerun on `2026-05-16`, the runtime emitted fresh payload rows:

- `v3_rows=71`
- `full_snapshot_payload_rows=71`
- `policy_hash_unique_count=1`
- `stale_against_config=false`

However, strict replay failed:

- `replay_status=fail_closed`
- `status_counts.payload_hash_mismatch=71`
- `--strict` exit code `2`

This means the payload exists, but the logged `v3_feature_snapshot_hash` does not match the validator hash recomputed from `v3_materialized_feature_snapshot`.

## Decision

Do not accept P3.2 full replay readiness until the logged feature snapshot hash is proven to be computed over the exact replayable representation consumed by the validator.

The canonical V3 replay boundary is the persisted replay payload, not an equivalent-looking in-memory `MaterializedFeatureSet`.

For future P3.2 remediation, the implementation must satisfy one of these equivalent contracts:

1. compute `v3_feature_snapshot_hash` from the exact serialized `v3_materialized_feature_snapshot` payload that is written to JSONL, or
2. construct the persisted payload from a canonical decoded structure and compute the hash from that exact decoded structure before logging, with a test that uses a production-shaped payload and a runtime-shaped row, or
3. persist an explicit canonical payload/hash envelope whose bytes are used by both runtime and validator.

The validator remains fail-closed. `v3_shadow_report.py` reporting `replay_status=full` only means payload presence; it is not sufficient for full replay readiness. The acceptance gate is `v3_full_replay_report.py --strict` returning exit `0` with `replay_status=full_replay_ok`.

## Consequences

Positive:

- prevents false approval of payload rows that cannot be replayed deterministically
- preserves auditability and replay semantics
- prevents P3/P2 escalation based on non-replay-stable evidence

Negative:

- P3.2 remains blocked despite fresh rows and payload presence
- another technical fix and clean rerun are required

## Non-goals

This ADR does not authorize:

- P2 promotion
- active V2/V2.5 policy changes
- V3 scoring or threshold tuning
- IWIM changes
- live sender or execution changes

## Operational Reference

Primary evidence:

- `PLANS/AUDYT/RAPORT_OPERACYJNY_P3_2_V3_FULL_REPLAY_R2_20260516.md`
- `logs/rollout/shadow-burnin-v3-p32-replay-r2/decisions/shadow-burnin-v3-p32-replay-r2/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl`

Failed gate:

```bash
python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p32-replay-r2.toml \
  --strict --json
```

Observed result:

```text
status=fail_closed
replay_status=fail_closed
v3_rows=71
payload_hash_mismatch=71
```
