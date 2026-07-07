# Raport Shadow V2 L2-G: memory-bounded validation harness 20260707

## Status

```text
stage=L2-G_MEMORY_BOUNDED_VALIDATION_HARNESS
final_verdict=L2_G_MEMORY_BOUNDED_VALIDATION_HARNESS_READY
runtime_decision_behavior_changes=NONE
gatekeeper_policy_changes=NONE
buy_reject_changes=NONE
selector_runtime_changes=NONE
tx_jito_live_path_changes=NONE
provider_stream_changes=NONE
threshold_changes=NONE
active_close_changes=NONE
shadow_close_only_changes=NONE
approval_flags=false
```

## Paused Run

The active compact research run was paused before code changes:

```text
run_id=shadow-v2-l2-f-research-codex-20260707-r4-compact-headroom-12h
pid=1935151
signal=SIGSTOP
state=T (stopped)
wrapper_config=/root/Gho/configs/rollout/shadow-v2-l2-f-research-codex-20260707-r4-compact-headroom-12h.local.toml
brain_config=/root/Gho/configs/rollout/ghost_brain_shadow_v2_l2_f_research_codex_20260707_r4_compact_headroom_12h.local.toml
report_root=/root/Gho/reports/selector/shadow-v2-l2-f-research-codex-20260707-r4-compact-headroom-12h
```

Observed memory at pause:

```text
VmRSS=10908396 kB
RssAnon=10900104 kB
RssFile=8292 kB
VmData=12500532 kB
VmSwap=0 kB
Threads=21
```

The process is still the old release binary. It must be restarted/rebuilt to
benefit from this fix.

## Leak Classification

This is not evidence of filesystem cache growth, file descriptor growth, or
thread leakage. The growth is anonymous heap and comes from an unbounded
retention design in the validation harness:

```text
ShadowV2ValidationHarness
  -> JsonlShadowV2CanonicalWriter
  -> ShadowV2CanonicalEventStream.events: Vec<ShadowPositionEventV2>
```

`append_record()` emitted compact artifacts, but every canonical event remained
in `events` for the entire process lifetime. Therefore compact disk emission
did not bound RSS.

## Fix

After a terminal event is durably flushed and derived evidence is complete, the
harness evicts that closed position's canonical events from memory:

```text
TERMINAL_TRUTH
replay_write=Ok
lifecycle_write=Ok
density_write=Ok
validation_evidence_status=Complete
compact_density_enabled=true
replay_lifecycle_compact_refs_enabled=true
density_full_stream_enabled=false
```

The eviction is memory-only. Durable artifacts still contain the complete
evidence:

```text
shadow_position_event_v2.jsonl
shadow_replay_v2.jsonl
shadow_lifecycle_v2.jsonl
shadow_path_density_v2.jsonl
shadow_source_ref_manifest_v2.jsonl
```

The harness keeps ordering/terminal guard maps:

```text
seen_event_ids
terminal_event_by_position
last_process_seq_by_position
```

Late appends for an evicted terminal position fail closed:

```text
HARNESS_POSITION_EVICTED_AFTER_TERMINAL_FLUSH
```

This prevents creating a partial new in-memory history for a position that was
already terminal-flushed.

## Shadow V2 Quality Impact

Evidence quality is preserved:

- canonical JSONL is written before eviction;
- replay/lifecycle compact rows keep count/range/hash evidence;
- density compact rows for declared horizons are written before eviction;
- source-ref manifests remain the audit path for compact refs;
- full-density stream mode is not evicted by default;
- no L2 declared horizon is removed;
- no audit predicate is weakened.

The change reduces only hot RAM retention for already closed positions.

## Verification

Regression test:

```text
shadow_v2_l2_g_terminal_flush_evicts_closed_position_events_from_memory_only
```

The test proves that:

```text
canonical JSONL retains all 125 rows
replay/lifecycle compact rows retain source_event_count=125
density emits 5 declared horizon rows
in-memory events_for_position(pos-a) becomes empty after terminal flush
terminal_event_id(pos-a) remains available
late post-terminal event fails closed
```

Executed checks:

```bash
cargo test -p ghost-brain shadow_v2_l2_g_terminal_flush_evicts_closed_position_events_from_memory_only -- --nocapture
cargo test -p ghost-brain shadow_v2_l2_g -- --nocapture
```

The broad `shadow_v2_l2_g` filter executed 10 matching tests and passed.

## Final Verdict

```text
L2_G_MEMORY_BOUNDED_VALIDATION_HARNESS_READY
```
