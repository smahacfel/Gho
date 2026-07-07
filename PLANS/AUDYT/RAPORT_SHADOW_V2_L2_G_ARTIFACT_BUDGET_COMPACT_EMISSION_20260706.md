# Raport Shadow V2 L2-G: artifact budget and compact evidence emission 20260706

## Status

```text
stage=L2-G_ARTIFACT_BUDGET_COMPACT_EMISSION
final_verdict=L2_G_ARTIFACT_BUDGET_COMPACT_EMISSION_READY
runtime_decision_behavior_changes=NONE
gatekeeper_policy_changes=NONE
buy_reject_changes=NONE
selector_runtime_changes=NONE
tx_jito_live_path_changes=NONE
provider_stream_changes=NONE
active_close_changes=NONE
shadow_close_only_changes=NONE
approval_flags=false
```

L2-G jest operacyjnym hardeningiem L2. Nie jest L3, nie przyznaje runtime
approval i nie zmienia decyzji tradingowych. Celem jest zatrzymanie eksplozji
artefaktow Shadow V2 przy kolejnych profile/research runach.

## Root Cause

Oversized run:

```text
reports/selector/shadow-v2-l2-f-research-codex-20260706-r1
```

Root cause mial trzy warstwy:

1. `ShadowV2ValidationHarness::append_record()` po kazdym canonical append
   emitowal replay, lifecycle i density snapshots.
2. Density stream emitowal kazdy snapshot dla 7 horyzontow, wlacznie z
   niezadeklarowanymi long horizons `300000` i `500000`.
3. Replay/lifecycle wielokrotnie przepisywaly pelne historie referencji:
   `source_event_ids`, `path_sample_event_ids` oraz `canonical_event:*` w
   `envelope.source_refs`.

Dodatkowo profil logowania dopuszczal verbose stdout/system spam, m.in. relay
diagnostics, enrichment logs i per-event router logs.

## Byte / Row Amplification

R1 zostal usuniety przez operatora przed finalnym raportem L2-G. Ponizsze dane
pochodza z pomiarow `stat`, `du` i row-countow zebranych przed cleanupem.

| artifact | size_bytes | row_count | avg_bytes_per_row | dominant amplification |
|---|---:|---:|---:|---|
| `shadow_path_density_v2.jsonl` | 13086613504 | 1508430 | 8675.65 | 7 horizon rows per append, repeated `source_path_sample_event_ids` |
| `shadow_replay_v2.jsonl` | 9681437395 | 215491 | 44927.34 | repeated `source_event_ids`, `path_sample_event_ids`, `canonical_event:*` refs |
| `shadow_lifecycle_v2.jsonl` | 8004169728 | 215490 | 37144.04 | repeated `source_event_ids`, `canonical_event:*` refs |
| `shadow_position_event_v2.jsonl` | 1295433728 | 215491 | 6011.54 | canonical append-only stream |
| `launcher.stdout.log` | 1541640192 | n/a | n/a | verbose runtime stdout logging |
| `system.log.2026-07-06` | 1259556864 | n/a | n/a | verbose system logging |

Directory-level measured sizes:

```text
reports/selector/shadow-v2-l2-f-research-codex-20260706-r1 = 32067655158 bytes
logs/rollout/shadow-v2-l2-f-research-codex-20260706-r1 = 4027828594 bytes
datasets/events/shadow-v2-l2-f-research-codex-20260706-r1 = 680636960 bytes
logs/shadow_v2/shadow-v2-l2-f-research-codex-20260706-r1 = 18116608 bytes
```

Density amplification details:

```text
density_rows=1508430
rows_per_horizon=215490
horizons_emitted=7
declared_horizons=2000,3000,10000,30000,120000
undeclared_horizons=300000,500000
```

## Implemented Fixes

### 1. Density compact mode

Default L2 density now emits only declared baseline horizons:

```text
2000
3000
10000
30000
120000
```

Long horizons are not emitted by default:

```text
300000 = opt-in only
500000 = opt-in only
```

Full stream opt-in:

```text
SHADOW_V2_DENSITY_FULL_STREAM=1
```

Default compact density writes only a final terminal snapshot per
`position_id + horizon_ms`. It does not flush on `EXIT_FILL`; it waits for
`TERMINAL_TRUTH`, preventing duplicate compact rows when both exit fill and
terminal truth exist.

R1-like density estimate:

```text
before_rows=1508430
after_rows_estimate=5480
before_bytes=13086613504
after_bytes_estimate=47542572
```

The estimate uses R1 terminal positions:

```text
terminal_positions=1096
declared_horizons=5
1096 * 5 = 5480 rows
```

### 2. Replay/lifecycle compact provenance

Replay/lifecycle rows no longer repeat full source histories in every derived
row under default compact mode.

Instead, compact rows carry:

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

The shared manifest is:

```text
shadow_source_ref_manifest_v2.jsonl
```

The manifest row stores count/range/hash metadata for the canonical source
range. It does not repeat the full arrays in every replay/lifecycle row.

### 3. Artifact budget guard

Default L2 profile budget:

```text
max_total_artifact_bytes=5368709120
max_file_bytes=2147483648
max_rows_per_file=2000000
max_density_rows=250000
max_stdout_bytes=268435456
max_system_log_bytes=536870912
```

Budget breach is fail-closed:

```text
BLOCKED_L2_ARTIFACT_BUDGET_EXCEEDED
```

The harness checks configured Shadow V2 artifacts before writes and returns a
typed write failure instead of continuing unbounded artifact emission.

### 3B. Disk-headroom profile budget for long research runs

R2 compact research run exposed a second budget bug: the original
`max_total_artifact_bytes=5368709120` stopped Shadow V2 writes while the
filesystem still had tens of GB free. That is correct for a small iterative
budget, but wrong for a deliberate 12h research profile.

L2-G now supports disk-headroom budgeting:

```text
artifact_budget_disk_headroom_enabled=true
max_total_artifact_bytes=0
min_free_disk_bytes=3221225472
max_density_rows=0
```

In this mode `max_total_artifact_bytes=0` means "no fixed configured-artifact
cap", and `max_density_rows=0` means "no fixed density row cap". Both zero
values are legal only when disk-headroom budgeting is enabled. The harness
checks real filesystem free space via `statvfs` before each Shadow V2 artifact
write and fails closed only when available bytes are at or below the configured
free-space margin.

The local 12h compact research profile was updated at:

```text
configs/rollout/ghost_brain_shadow_v2_l2_f_research_codex_20260706_r2_compact.local.toml
```

with:

```text
artifact_budget_disk_headroom_enabled=true
max_total_artifact_bytes=0
min_free_disk_bytes=3221225472
max_density_rows=0
```

This preserves simulation quality: declared L2 density horizons remain
`2000,3000,10000,30000,120000`, full density stream remains opt-in only, and no
Gatekeeper/BUY/REJECT/selector/TX/Jito/live path is changed.

### 3C. Controlled shutdown on budget breach

Before this fix, a budget breach could make canonical Shadow V2 writes fail
while the process kept running and emitted repeated `CanonicalWriteFailed`
warnings. L2-G now marks the typed artifact-budget blocker globally and the
launcher has a Shadow V2 artifact-budget guard that requests controlled global
shutdown when:

```text
BLOCKED_L2_ARTIFACT_BUDGET_EXCEEDED
```

is observed.

This prevents half-dead runs: the process no longer keeps collecting after the
Shadow V2 evidence surface has become incomplete due to resource exhaustion.

### 3D. Memory-bounded compact validation harness

The 12h compact-headroom research run exposed a separate RAM problem: compact
disk emission did not compact the in-memory canonical event stream. The paused
process showed:

```text
run_id=shadow-v2-l2-f-research-codex-20260707-r4-compact-headroom-12h
pid=1935151
state=T (stopped)
VmRSS=10908396 kB
RssAnon=10900104 kB
VmSwap=0 kB
```

The root cause was lifetime retention of all canonical events in:

```text
ShadowV2ValidationHarness
  -> JsonlShadowV2CanonicalWriter
  -> ShadowV2CanonicalEventStream.events
```

L2-G now evicts a closed position's canonical events from RAM after terminal
truth and all derived compact evidence have been durably written:

```text
canonical_write=Ok
replay_write=Ok
lifecycle_write=Ok
density_write=Ok
validation_evidence_status=Complete
```

The eviction is enabled only for compact evidence mode:

```text
compact_density_enabled=true
replay_lifecycle_compact_refs_enabled=true
density_full_stream_enabled=false
```

Durable artifacts are preserved. Late post-terminal appends for an evicted
position fail closed with:

```text
HARNESS_POSITION_EVICTED_AFTER_TERMINAL_FLUSH
```

This bounds RAM primarily by currently open positions plus guard maps, without
weakening Shadow V2 evidence quality.

### 4. JSONL rotation manifest

Large Shadow V2 JSONL artifacts now rotate before a per-file budget breach.
The active file remains the configured path and completed parts are written as:

```text
<artifact>.part-000001.jsonl
<artifact>.part-000002.jsonl
...
```

Rotation metadata is written to:

```text
shadow_artifact_rotation_manifest_v2.jsonl
```

Each rotation manifest row records:

```text
path/logical_path
rotated_path
uncompressed_size_bytes
row_count
hash_algorithm=blake3
hash_uncompressed
rotation_index
rotated_at_wall_ms
```

Compression is not enabled in L2-G; `compressed_path`,
`compressed_size_bytes`, and `hash_compressed` remain nullable. This satisfies
the storage requirement through rotation rather than `.jsonl.zst`.

### 5. Logging profile

New research-run profile:

```text
SHADOW_V2_LOG_PROFILE=l2_research_compact
```

The profile suppresses broad info/debug targets for launcher/seer/post-buy/
trigger/oracle/brain paths and preserves warnings/errors. It is a logging
filter profile only; it does not change runtime decision behavior.

### 6. Manifest/schema contract

`shadow_source_ref_manifest_v2.jsonl` is now a required post-run artifact in
the manifest contract. The schema manifest documents compact replay/lifecycle
fields and explicitly allows `source_event_ids` / `path_sample_event_ids` to be
empty when the corresponding manifest refs and range hashes are present.

`shadow_artifact_rotation_manifest_v2.jsonl` is an optional post-run artifact.
It is present only when a configured Shadow V2 JSONL artifact rotates. The
schema manifest documents its path, size, row-count, BLAKE3 hash and rotation
index fields.

## Compression / Rotation

L2-G does not add a runtime `.jsonl.zst` writer. The enforced default is:

```text
compact emission + JSONL rotation + artifact budget fail-closed
```

This means the default L2 profile no longer depends on post-hoc compression to
avoid disk exhaustion. If a future full-stream diagnostic run needs raw stream
retention beyond the budget, it may add compressed rotation as a separate
storage optimization, but L2-G already prevents single-file JSONL blowups by
rotating active artifacts and recording rotation integrity metadata.

## L2 Audit Compatibility

Compact mode preserves L2-F audit inputs:

- temporal audit: canonical stream remains unchanged;
- density/retention audit: compact density contains declared horizon rows and
  latest/final terminal snapshots;
- Gatekeeper denominator audit: unchanged;
- manifest audit: source ref manifest is now required and sha/row-counted;
- rotation manifest audit: optional artifact documents rotated part integrity
  when rotation occurs;
- replay/lifecycle audit: terminal fields and canonical joins remain present;
- account data hash coverage: unchanged canonical evidence path;
- evidence-complete scope: compact density is still per position/horizon.
- memory-bounded harness: evicts only after complete terminal evidence flush,
  so audits consume durable artifacts rather than hot in-memory history.

Compact mode removes repeated history payloads, not the proof surface.

## Oversized Run Recommendation

Observed R1 did not complete cleanly. It had disk-full symptoms:

```text
CanonicalWriteFailed
DerivedArtifactWriteFailed
Shadow V2 runtime path sample append incomplete
```

It also had malformed/truncated rows caused by disk exhaustion.

Recommended action before cleanup was:

```text
quarantine or delete after manifesting summary
```

Current state:

```text
reports/selector/shadow-v2-l2-f-research-codex-20260706-r1 = deleted_by_operator
```

It should not be used for L2 acceptance claims.

## Required Artifacts

```text
PLANS/AUDYT/RAPORT_SHADOW_V2_L2_G_ARTIFACT_BUDGET_COMPACT_EMISSION_20260706.md
docs/ADR/ADR_8D_SHADOW_V2_L2_G_ARTIFACT_BUDGET_COMPACT_EMISSION_20260706.md
reports/selector/shadow_v2_l2_g_artifact_budget_compact_emission_summary.csv
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

## Final Verdict

```text
L2_G_ARTIFACT_BUDGET_COMPACT_EMISSION_READY
```
