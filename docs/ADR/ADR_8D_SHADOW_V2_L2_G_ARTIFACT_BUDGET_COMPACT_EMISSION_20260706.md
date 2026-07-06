# ADR-8D: Shadow V2 L2-G Artifact Budget Compact Emission 20260706

## Status

Accepted implementation stage.

## Decision

Wprowadzamy L2-G artifact budget and compact evidence emission jako
operacyjny hardening L2:

```text
final_verdict=L2_G_ARTIFACT_BUDGET_COMPACT_EMISSION_READY
```

Decyzja:

1. Default L2 density output jest compact i emituje tylko declared horizons:
   `2000, 3000, 10000, 30000, 120000`.
2. Long horizons `300000` i `500000` sa full-stream opt-in only przez:
   `SHADOW_V2_DENSITY_FULL_STREAM=1`.
3. Replay/lifecycle provenance uzywa compact range/hash fields oraz
   `shadow_source_ref_manifest_v2.jsonl`, zamiast powtarzac pelne arrays w
   kazdym derived row.
4. L2 profile ma fail-closed artifact budget z typed blockerem:
   `BLOCKED_L2_ARTIFACT_BUDGET_EXCEEDED`.
5. Large JSONL artifacts rotate to `.part-000001.jsonl` style files before a
   per-file budget breach and write integrity metadata to
   `shadow_artifact_rotation_manifest_v2.jsonl`.
6. `SHADOW_V2_LOG_PROFILE=l2_research_compact` ogranicza verbose stdout/system
   logging dla research runow.

## Context

Krotki L2 research profile run `shadow-v2-l2-f-research-codex-20260706-r1`
wygenerowal ok. 30-40GB artefaktow, glownie:

```text
shadow_path_density_v2.jsonl
shadow_replay_v2.jsonl
shadow_lifecycle_v2.jsonl
shadow_position_event_v2.jsonl
launcher.stdout.log
system.log.*
```

Najwiekszy mechanizm wzrostu byl deterministyczny: harness emitowal derived
snapshots po kazdym canonical append, density robilo to dla 7 horyzontow, a
replay/lifecycle powtarzaly narastajace historie ID.

## Implemented Contract

Default density horizons:

```text
2000
3000
10000
30000
120000
```

Full density stream:

```text
SHADOW_V2_DENSITY_FULL_STREAM=1
```

Default budgets:

```text
max_total_artifact_bytes=5368709120
max_file_bytes=2147483648
max_rows_per_file=2000000
max_density_rows=250000
max_stdout_bytes=268435456
max_system_log_bytes=536870912
```

Compact replay/lifecycle fields:

```text
source_event_count
source_event_first_id
source_event_last_id
source_event_range_hash
source_event_manifest_ref
path_sample_count
path_sample_first_id
path_sample_last_id
path_sample_range_hash
path_sample_manifest_ref
```

Manifest:

```text
shadow_source_ref_manifest_v2.jsonl
```

Rotation manifest:

```text
shadow_artifact_rotation_manifest_v2.jsonl
```

Rotated JSONL parts:

```text
<artifact>.part-000001.jsonl
<artifact>.part-000002.jsonl
```

Rotation rows record path, rotated path, uncompressed size, row count,
`hash_algorithm=blake3`, `hash_uncompressed`, rotation index and wall-clock
time. Compression fields remain nullable because L2-G uses rotation rather
than `.jsonl.zst`.

## Code-Level Changes

### `ghost-brain/src/guardian/post_buy/shadow_v2.rs`

- Adds L2 declared/undeclared horizon constants.
- Adds `SHADOW_V2_DENSITY_FULL_STREAM` opt-in.
- Adds `ShadowV2ArtifactBudgetConfig`.
- Adds `ShadowV2SourceRefManifestRow`.
- Adds `ShadowV2ArtifactRotationManifestRow`.
- Adds BLAKE3 range hashes for compact provenance.
- Changes default density to compact terminal-only emission.
- Rotates large JSONL artifacts before per-file budget breach.
- Adds fail-closed budget checks before Shadow V2 artifact writes.
- Compacts replay/lifecycle source refs under default harness config.

### `ghost-brain/src/config/ghost_brain_config.rs`

- Adds backward-compatible `#[serde(default)]` fields for compact density,
  compact replay/lifecycle refs, artifact budgets, artifact rotation and log
  profile.
- Validates incompatible `density_full_stream_enabled` +
  `compact_density_enabled`.

### `ghost-launcher/src/main.rs`

- Adds `SHADOW_V2_LOG_PROFILE=l2_research_compact` logging filter.
- Preserves warnings/errors and suppresses broad info/debug spam for compact
  research runs.

### Manifest contracts

- Adds `shadow_source_ref_manifest_v2.jsonl` to the artifact contract.
- Adds optional `shadow_artifact_rotation_manifest_v2.jsonl` to the artifact
  contract.
- Documents compact replay/lifecycle fields in the required schema manifest.
- Documents rotation manifest fields in the required schema manifest.

## Rejected Alternatives

### Keep full density stream as default

Rejected. It recreated 7 horizon rows after every append and emitted
undeclared 300s/500s horizons by default.

### Keep full source refs in every replay/lifecycle row

Rejected. It made derived rows grow with position history and repeated the same
canonical IDs thousands of times.

### Treat disk-full R1 as usable L2 evidence

Rejected. R1 had write failures and malformed/truncated rows. It is not a valid
L2 acceptance scope.

### Add runtime zstd writer in this PR

Rejected for this stage. L2-G implements runtime rotation plus fail-closed
budgets. `.jsonl.zst` compression can be added later for explicit full-stream
diagnostics, but default L2 profile does not require post-hoc compression to
avoid disk exhaustion.

## Consequences

1. Default future L2 profile runs emit compact density instead of unbounded
   density snapshots.
2. Replay/lifecycle rows remain audit-compatible but no longer repeat full ref
   histories.
3. Artifact budgets fail closed before disk exhaustion.
4. Large JSONL artifacts rotate with BLAKE3 integrity metadata.
5. Compact logging profile reduces stdout/system log growth.
6. No trading decision behavior changes.

## Compatibility

Schema changes are additive. Existing consumers that read replay/lifecycle
terminal fields continue to work. Audits that require full source ID arrays
must use `shadow_source_ref_manifest_v2.jsonl` plus range hash fields.
Audits that inspect rotated parts use `shadow_artifact_rotation_manifest_v2.jsonl`.

Runtime decision behavior is unchanged.

## Verification

Expected checks:

```bash
cargo test -p ghost-brain shadow_v2_l2_g -- --nocapture
cargo test -p ghost-launcher shadow_v2_l2_research_compact_log_profile -- --nocapture
python3 scripts/test_shadow_v2_manifest_audit.py
python3 tests/test_shadow_v2_path_density_horizon_audit.py
cargo fmt --check
git diff --check
git diff --cached --check
```

## Approval Flags

```text
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
```

## Final Decision

```text
L2_G_ARTIFACT_BUDGET_COMPACT_EMISSION_READY
```
