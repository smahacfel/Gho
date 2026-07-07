# ADR-8D: Shadow V2 L2-G Memory-Bounded Validation Harness 20260707

## Status

Accepted implementation hardening.

## Decision

Shadow V2 L2 compact validation harness must not retain the full canonical
event history for positions whose terminal evidence has already been flushed.

The harness now evicts closed-position canonical events from in-memory state
only after all required derived evidence writes complete:

```text
canonical_write=Ok
replay_write=Ok
lifecycle_write=Ok
density_write=Ok
validation_evidence_status=Complete
event_kind=TERMINAL_TRUTH
compact_density_enabled=true
replay_lifecycle_compact_refs_enabled=true
density_full_stream_enabled=false
```

Eviction is memory-only. Durable JSONL artifacts remain unchanged:

```text
shadow_position_event_v2.jsonl
shadow_replay_v2.jsonl
shadow_lifecycle_v2.jsonl
shadow_path_density_v2.jsonl
shadow_source_ref_manifest_v2.jsonl
```

Late post-terminal appends for an evicted position fail closed with:

```text
HARNESS_POSITION_EVICTED_AFTER_TERMINAL_FLUSH
```

This prevents silently rebuilding incomplete per-position history after the
position was already terminal-flushed and evicted from RAM.

## Context

The compact artifact fix reduced disk growth, but the active long research run
still showed anonymous RSS growth. The paused process was:

```text
run_id=shadow-v2-l2-f-research-codex-20260707-r4-compact-headroom-12h
pid=1935151
state=T (stopped)
VmRSS=10908396 kB
RssAnon=10900104 kB
RssFile=8292 kB
VmSwap=0 kB
threads=21
```

The process had stable file-descriptor and thread counts. RSS was almost
entirely anonymous heap, not filesystem cache. The source was unbounded
retention by design:

```text
ShadowV2ValidationHarness
  -> JsonlShadowV2CanonicalWriter
  -> ShadowV2CanonicalEventStream
  -> events: Vec<ShadowPositionEventV2>
```

`append_record()` wrote compact artifacts, but still committed every canonical
event into the in-memory `events` vector for the whole process lifetime.
Compact disk output therefore did not imply compact RAM.

## Implementation Contract

The new memory boundary is:

```text
open position history retained in memory
closed position history flushed to durable evidence
closed position event vector evicted from memory
terminal id / ordering guard maps retained
late post-terminal events fail closed
```

The eviction gate is intentionally narrow. It is enabled only for the compact
L2 evidence profile. Full-density diagnostic mode keeps historical in-memory
events because it explicitly requests full-stream behavior.

The retained maps preserve terminal/order guard behavior:

```text
seen_event_ids
terminal_event_by_position
last_process_seq_by_position
```

## Non-Goals

This does not change:

```text
Gatekeeper policy
BUY/REJECT logic
selector runtime
TX/Jito/live path
provider streams
thresholds
shadow_close_only
active_close
runtime approval flags
```

This does not grant:

```text
runtime_approval
research_grade
live_equivalence
strategy_research_unblocked
shadow_close_only
active_close
```

## Consequences

Expected RAM behavior changes from lifetime-growth by all historical position
events to growth bounded primarily by active, non-terminal positions plus guard
maps. This preserves Shadow V2 evidence quality because all compact evidence is
written before eviction.

The currently paused run uses the old release binary. It should not be resumed
as proof of this fix. A restarted/rebuilt compact profile run is required to
observe the bounded-memory behavior.

## Verification

Added regression coverage:

```text
shadow_v2_l2_g_terminal_flush_evicts_closed_position_events_from_memory_only
```

The test proves:

```text
125 canonical JSONL rows remain durable
replay/lifecycle compact rows retain count/range/hash evidence
density emits declared compact rows
in-memory events_for_position is empty after terminal flush
terminal_event_id remains available
late post-terminal append fails closed
```

Checks:

```bash
cargo test -p ghost-brain shadow_v2_l2_g_terminal_flush_evicts_closed_position_events_from_memory_only -- --nocapture
cargo test -p ghost-brain shadow_v2_l2_g -- --nocapture
```

Both filters execute matching tests and pass.

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
L2_G_MEMORY_BOUNDED_VALIDATION_HARNESS_READY
```
