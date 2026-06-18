# R26_FSC_CANARY_FINAL_REPORT

Generated: 2026-06-13
Scope: `shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation`

## Purpose

R26 purpose:

```text
FSC full-chain canary + data collection only
```

This was not BUY validation.

This was not a positive validation of FSC as a hard policy signal.

This run only validates that the dedicated full-chain funding lane can be wired, that authoritative FSC runtime gates are emitted, and that fail-closed behavior works when the provider stream hiccups.

## Config

Config path:

```text
configs/rollout/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation.toml
```

Relevant config shape:

```text
funding_lane_mode = "full_chain"
execution_mode = "shadow"
entry_mode = "shadow_only"
```

FSC policy remained telemetry/shadow-only:

```text
fsc_v2.decision_enabled = false
fsc_v2.hard_reject_enabled = false
soft_penalty_high_fsc = 0
soft_penalty_high_fsc_high_cpv_combo = 0
enable_sybil_interference_layer = false
enable_sybil_combo_veto = false
gatekeeper_v3.evidence_requirements.fsc = false
```

No Gatekeeper policy, execution, send path, FSC veto, or FSC scoring change was made as part of this closeout.

## Expected Ingest Shape 4/4

Expected ingest sources:

1. raw gRPC primary: `grpc_global_stream`
2. raw gRPC funding lane: `grpc_funding_lane_full_chain`
3. NLN Program Stream: `solana.pump_fun.buy`
4. NLN Program Stream: `solana.pump_fun.buy_exact_sol_in`

Program streams are route/account evidence. They do not replace full-chain funding evidence.

## Actual Ingest Status

Final pre-stop metrics snapshot time:

```text
Sat Jun 13 00:11:20 UTC 2026
```

The run reached the intended 4/4 ingest shape:

```text
grpc_global_stream raw events:                 1,935,267
grpc_funding_lane_full_chain raw events:       6,311,725
solana.pump_fun.buy route evidence rows:          35,835
solana.pump_fun.buy_exact_sol_in rows:            14,628
NLN route evidence rows after stop:               51,871
```

No `ResourceExhausted` or explicit provider stream-limit rejection was observed in the closeout evidence reviewed.

## Provider Hiccup

The main provider incident occurred at:

```text
2026-06-12T23:57:06Z
```

Observed errors:

```text
NLN Program Streams receive error: h2 protocol error
primary_global gRPC error: h2 protocol error
funding_lane_full_chain gRPC error: h2 protocol error
funding_lane_full_chain stream ended
funding_lane_full_chain gRPC error: DataLoss, message: "lagged"
primary_global gRPC error: DataLoss, message: "lagged"
```

The incident hit both primary gRPC and funding lane transport, plus both NLN program-stream topics.

## Fail-Closed Behavior

At the provider hiccup, runtime set FSC unavailable and closed the authoritative coverage gate:

```text
available=false
warmup_ready=false
stream_available=false
coverage_window_ready=false
authoritative_buy_gate_open=false
coverage_window_remaining_ms=300000
source="seer_lane_health"
```

This is the expected fail-closed behavior.

Interpretation:

```text
R26_FSC_FAIL_CLOSED_PASS
```

## Recovery Behavior

The funding lane reconnected immediately after the hiccup:

```text
SUBSCRIBE_SENT profile=funding_lane_full_chain source_label=grpc_funding_lane_full_chain
Stream established
available=true
warmup_ready=true
coverage_window_ready=false
coverage_window_remaining_ms=300000
```

Because the authoritative stream was interrupted, the coverage horizon restarted from a full 300 seconds.

The coverage gate reopened at:

```text
2026-06-13T00:02:07.154961Z
coverage_window_ready=true
authoritative_buy_gate_open=true
coverage_window_remaining_ms=0
```

Interpretation:

```text
R26_FSC_CANARY_WIRING_PASS
R26_FSC_FAIL_CLOSED_PASS
```

## Final FSC Metrics

Final pre-stop FSC metrics:

```text
fsc_authoritative_buy_gate_open = 1
fsc_authoritative_funding_stream_available = 1
fsc_coverage_window_ready = 1
fsc_coverage_window_remaining_ms = 0
fsc_warmup_ready = 1

fsc_index_entries = 12,578
fsc_index_global_evictions_total = 476,033
fsc_index_per_recipient_overflows_total = 553,567

fsc_lookup_hit_rate = 0.05283362613052248
fsc_lookup_hits_total = 812
fsc_lookup_misses_total = 14,557
```

Final pre-stop Prometheus miss reasons:

```text
FSC_NO_RETAINED_RECIPIENT_HISTORY = 13,701
FSC_GLOBAL_RECIPIENT_EVICTED = 334
FSC_SAME_SLOT_ORDERING_UNAVAILABLE = 189
FSC_RELATIVE_FUNDING_TOO_SMALL = 166
FSC_ABS_ATTRIBUTION_TOO_SMALL = 124
FSC_LOW_ATTRIBUTION_CONFIDENCE = 33
FSC_NO_PREBUY_TRANSFER_IN_WINDOW = 10
```

## Decision-Level FSC Coverage

Post-stop closed artifact counts:

```text
gatekeeper_v2_decisions.jsonl rows = 2,907
```

Decision-level FSC status counts:

```text
clean = 36
degraded = 2,207
unavailable = 664
```

Decision-level `funding_source_concentration`:

```text
non_null = 36 / 2,907
min = 0.0
max = 1.0
avg = 0.7222222222222221
```

Decision-level coverage:

```text
overall known_coverage avg = 0.047311703434348984
clean known_coverage avg = 0.5912901912901912
degraded known_coverage avg = 0.052672711824742015
```

Interpretation:

```text
R26_FSC_COVERAGE_LOW
```

## BUY-Level FSC Coverage

Post-stop closed artifact counts:

```text
gatekeeper_v2_buys.jsonl rows = 354
shadow buys rows = 354
```

BUY-level FSC status counts:

```text
clean = 6
degraded = 348
unavailable = 0
```

BUY-level `funding_source_concentration`:

```text
non_null = 6 / 354
min = 0.3333333333333333
max = 1.0
avg = 0.7777777777777778
```

BUY-level coverage:

```text
known_coverage avg = 0.05232802646399464
known_buyers avg = 0.8559322033898306
total_buyers avg = 17.954802259887007
funding_lane_lag_slots avg = 11.290960451977401
funding_lane_lag_slots max = 99
```

BUY-level top degraded reasons:

```text
FSC_INSUFFICIENT_KNOWN_SOURCES = 335
CPV_COVERAGE_WINDOW_UNAVAILABLE = 174
SFD_PARTIAL_BALANCE_COVERAGE = 38
FSC_COVERAGE_WINDOW_UNAVAILABLE = 26
DES_NO_COMPARABLE_PAIRS = 22
SFD_NEGATIVE_BALANCE_DELTA_SKIPPED = 10
DBIA_NO_DEV_BUY = 9
FSC_SAME_SLOT_ORDERING_UNAVAILABLE = 7
FSC_LOW_ATTRIBUTION_CONFIDENCE = 6
```

BUY-level top miss reasons:

```text
FSC_NO_RETAINED_RECIPIENT_HISTORY = 5,677
FSC_GLOBAL_RECIPIENT_EVICTED = 132
FSC_RELATIVE_FUNDING_TOO_SMALL = 88
FSC_SAME_SLOT_ORDERING_UNAVAILABLE = 75
FSC_ABS_ATTRIBUTION_TOO_SMALL = 68
FSC_LOW_ATTRIBUTION_CONFIDENCE = 8
FSC_NO_PREBUY_TRANSFER_IN_WINDOW = 5
```

## Why Policy Signal Is Not Ready

FSC is not policy-ready in this run because:

- `funding_source_concentration` is non-null for only 36 / 2,907 decisions.
- `funding_source_concentration` is non-null for only 6 / 354 BUY rows.
- BUY-level average known coverage is about 5.23%.
- Lookup hit rate is about 5.28%.
- The dominant miss reason is `FSC_NO_RETAINED_RECIPIENT_HISTORY`.
- The index shows heavy capacity pressure: 476,033 global evictions and 553,567 per-recipient overflows.
- A provider hiccup reset the authoritative coverage window; fail-closed recovered correctly, but the event proves the signal must tolerate lane interruptions before promotion.

Required interpretation:

```text
R26_FSC_POLICY_SIGNAL_NOT_READY
R26_FSC_COVERAGE_LOW
```

## Shutdown

Shutdown sequence:

```text
2026-06-13T00:15Z: Ctrl+C / SIGINT sent through tmux
SIGINT did not complete promptly; runtime continued logging channel-closed funding-lane errors.
2026-06-13T00:16Z: SIGTERM sent to ghost-launcher process.
SIGKILL was not used.
```

Post-stop verification:

```text
tmux session stopped: yes
ghost-launcher process stopped: yes
stale selector R26 launcher/cargo process: no
```

The runtime log did not emit a clean `Ghost Launcher shutdown complete` marker before process exit. The final shutdown tail contains `Transport channel disconnected` and `Error processing funding-lane event: Failed to send on channel: Channel send failed: channel closed` while the process was draining after SIGINT.

## Final Log Paths

Runtime log:

```text
reports/selector/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/run_lifecycle_guard_20260612T215637Z/runtime.log
```

Decision log:

```text
logs/rollout/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/decisions/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/v2.2/legacy_live/5c0f6d951240d1c0c9bed52c782a1e47f1de5f15d600064ba347e762a170576d/gatekeeper_v2_decisions.jsonl
```

BUY log:

```text
logs/rollout/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/decisions/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/v2.2/legacy_live/5c0f6d951240d1c0c9bed52c782a1e47f1de5f15d600064ba347e762a170576d/gatekeeper_v2_buys.jsonl
```

Shadow buys:

```text
logs/shadow_run/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation-buys.jsonl
```

NLN route evidence:

```text
logs/nln_capture/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/route_manifest_evidence_candidates_v1.jsonl
```

## Disk And Log Size Warning

Pre-stop disk:

```text
/root/Gho: 150G size, 125G used, 19G available, 87% used
```

Post-stop disk:

```text
/root/Gho: 150G size, 126G used, 19G available, 88% used
```

Large artifacts after stop:

```text
runtime.log = 1,240,496,498 bytes
gatekeeper_v2_decisions.jsonl = 208,892,142 bytes
gatekeeper_v2_buys.jsonl = 26,799,907 bytes
shadow buys = 1,192,131 bytes
NLN route evidence = 234,731,185 bytes
```

Do not run another unbounded R26/R27 canary without a separate start decision and disk budget.

## Final Verdict

```text
FSC_FULLCHAIN_CANARY_PARTIAL_PASS_WIRING_FAIL_COVERAGE

WIRING_PASS
FAIL_CLOSED_PASS
COVERAGE_NOT_POLICY_READY
```

Operational decision labels:

```text
R26_FSC_CANARY_WIRING_PASS
R26_FSC_FAIL_CLOSED_PASS
R26_FSC_POLICY_SIGNAL_NOT_READY
R26_FSC_COVERAGE_LOW
```

Next possible stage, not started here:

```text
R26B_FSC_RETENTION_CANARY
```

R26B must not start without separate approval and a bounded retention/capacity hypothesis.
