# R26B FSC Retention Canary Final Report

## Purpose

purpose: FSC retention/capacity delta canary + coverage data collection only

This is not BUY validation. R26B did not validate FSC as a hard policy signal, score input, veto, execution input, or send-path input.

## Config

- rollout: `shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary`
- config path: `configs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary.toml`
- brain config path from rows: `/root/Gho/configs/rollout/ghost_brain_selector_dataset_sampler.toml`
- funding_lane_mode = `full_chain`
- FSC V2 active thresholds observed in rows:
  - `min_abs_store_lamports = 1000000`
  - `min_abs_attribution_lamports = 10000000`
  - `min_rel_to_buy = 0.20`
  - `ttl_seconds = 300`
- R26B retention/capacity delta:
  - `fsc_global_recipient_cap = 50000`
  - `fsc_per_recipient_cap = 256`

## Stop Confirmation

Final snapshot was taken before stop. R26B was then stopped without SIGKILL:

- tmux session: stopped, `tmux ls` returned no sessions
- stale process check: no matching `selector_r26b_fsc_retention_canary`, `shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary`, launcher, or `ghost-launcher` process remained
- metrics listener: no listener on `:9128`
- final disk after stop:

```text
/dev/sda1  150G  129G   16G  90% /
```

Final log/artifact sizes:

```text
487M   reports/selector/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary
1020M  logs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary
1.1M   logs/shadow_run/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary
299M   logs/nln_capture/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary
```

Final log paths:

- `logs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/oracle.log.2026-06-13`
- `logs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/system.log.2026-06-13`
- `logs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/decisions/.../gatekeeper_v2_decisions.jsonl`
- `logs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/decisions/.../gatekeeper_v2_buys.jsonl`
- `logs/nln_capture/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/`

## Expected Ingest Shape

expected ingest shape: 4/4

Expected shape for this experiment was primary raw gRPC + second raw gRPC funding lane + decoded/program stream coverage, with full-chain funding lane enabled. The canary was explicitly data collection only.

## Actual Ingest Status

Final scrape and logs show the lane was mechanically active, but durable raw transfer artifacts are incomplete for attribution autopsy:

- `seer_events_received_total{source="grpc_funding_lane_full_chain"} = 2924634`
- `seer_events_received_total{source="grpc_global_stream"} = 834919`
- watchdog near stop: `grpc_state=CONNECTED reconnects=0`
- snapshot listener near stop: `lagged=0`
- final scrape gauge: `seer_grpc_connection_status = 0`

The scrape gauge conflicts with the final watchdog lines and event counters. I treat the watchdog plus event counters as stronger evidence that the hot path was active, while recording the gauge inconsistency as a telemetry issue.

NLN capture counts:

```text
funding_events_v1.jsonl                 0
system_transfers_raw_v1.jsonl           0
nln_pumpfun_buy_exact_sol_in_raw_v1     3297
nln_pumpfun_buy_raw_v1                  8550
raw_pumpfun_instruction_evidence_v1     31385
route_manifest_evidence_candidates_v1   11847
```

Interpretation: runtime funding lane was active, but the raw funding transfer artifacts needed for offline buyer-wallet attribution were not persisted.

## Provider And Runtime Errors

No provider failure pattern was found in R26B final logs for:

- `ResourceExhausted = 0`
- `DataLoss = 0`
- `h2 protocol = 0`
- `protocol error = 0`
- `HTTP/2 = 0`
- `RST_STREAM = 0`
- `reconnect storm = 0`

Reconnect/watchdog status:

- watchdog lines counted: 72
- `grpc_state=CONNECTED`: 70
- final watchdog examples show `reconnects=0`
- snapshot listener examples show `lagged=0`

Shutdown noise after stop:

- `Transport channel disconnected`: present after controlled stop
- `Failed to send on channel: Channel send failed: channel closed`: present after controlled stop

These shutdown errors are not evidence of a runtime reconnect storm during the canary hot path.

## Final FSC Metrics

Final scrape:

```text
fsc_lookup_hit_rate = 0.05282583621683968
fsc_lookup_hits_total = 229
fsc_lookup_misses_total = 4106
fsc_index_entries = 12403
fsc_index_global_cap_evictions_total = 0
fsc_index_window_prunes_total = 137223
fsc_index_per_recipient_overflows_total = 113379
fsc_index_estimated_memory_bytes = 1222439680
fsc_evidence_status_total{status="clean"} = 5
fsc_evidence_status_total{status="degraded"} = 595
fsc_evidence_status_total{status="unavailable"} = 151
```

Top runtime lookup miss reasons from final scrape:

```text
FSC_NO_RETAINED_RECIPIENT_HISTORY = 3990
FSC_ABS_ATTRIBUTION_TOO_SMALL = 58
FSC_RELATIVE_FUNDING_TOO_SMALL = 52
FSC_LOW_ATTRIBUTION_CONFIDENCE = 4
FSC_NO_PREBUY_TRANSFER_IN_WINDOW = 2
```

## Decision-Level FSC Coverage

Durable decision rows after excluding stale pre-valid-start rows:

- decision file: `logs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/decisions/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/v2.2/legacy_live/76efbfe40d091ef91a1fa9e83d653267b4666fb291df33eecce6d8ab962d801a/gatekeeper_v2_decisions.jsonl`
- total lines: 803
- excluded pre-valid-start rows: 40
- valid rows: 763
- FSC status counts:
  - clean: 5
  - degraded: 606
  - unavailable: 152
- clean decision FSC coverage: 5 / 763 = 0.66%
- `funding_source_concentration` non-null rows: 5
- summed buyer samples: 4478
- summed known buyers: 232
- summed unknown buyers: 4246

Top decision-level miss reasons:

```text
FSC_NO_RETAINED_RECIPIENT_HISTORY = 4123
FSC_ABS_ATTRIBUTION_TOO_SMALL = 63
FSC_RELATIVE_FUNDING_TOO_SMALL = 54
FSC_LOW_ATTRIBUTION_CONFIDENCE = 4
FSC_NO_PREBUY_TRANSFER_IN_WINDOW = 2
```

Top degraded reasons:

```text
fsc_evidence_partial = 758
FSC_INSUFFICIENT_KNOWN_SOURCES = 754
SFD_INSUFFICIENT_BUYS = 392
DES_INSUFFICIENT_BUYS = 358
FTDI_INSUFFICIENT_BUYS = 339
CPV_INSUFFICIENT_SIGNERS = 339
CPV_COVERAGE_WINDOW_UNAVAILABLE = 238
DBIA_NO_DEV_BUY = 174
DBIA_INSUFFICIENT_BUYERS = 171
SFD_PARTIAL_BALANCE_COVERAGE = 110
```

## BUY-Level FSC Coverage

Durable BUY rows after excluding stale pre-valid-start rows:

- buy file: `logs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/decisions/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary/v2.2/legacy_live/76efbfe40d091ef91a1fa9e83d653267b4666fb291df33eecce6d8ab962d801a/gatekeeper_v2_buys.jsonl`
- total lines: 144
- excluded pre-valid-start rows: 8
- valid BUY rows: 136
- BUY FSC status counts:
  - clean: 4
  - degraded: 132
  - unavailable: 0
- clean BUY FSC coverage: 4 / 136 = 2.94%
- BUY `funding_source_concentration` non-null rows: 4
- summed BUY buyer samples: 2265
- summed BUY known buyers: 123
- summed BUY unknown buyers: 2142

Top BUY-level miss reasons:

```text
FSC_NO_RETAINED_RECIPIENT_HISTORY = 2070
FSC_ABS_ATTRIBUTION_TOO_SMALL = 38
FSC_RELATIVE_FUNDING_TOO_SMALL = 32
FSC_NO_PREBUY_TRANSFER_IN_WINDOW = 2
```

Top BUY degraded reasons:

```text
FSC_INSUFFICIENT_KNOWN_SOURCES = 132
fsc_evidence_partial = 132
CPV_COVERAGE_WINDOW_UNAVAILABLE = 72
SFD_PARTIAL_BALANCE_COVERAGE = 15
FSC_COVERAGE_WINDOW_UNAVAILABLE = 13
DES_NO_COMPARABLE_PAIRS = 7
SFD_NEGATIVE_BALANCE_DELTA_SKIPPED = 2
DBIA_NO_DEV_BUY = 1
```

## R26B Vs R26

R26 baseline:

- valid decision rows: 2907
- clean decision FSC coverage: 36 / 2907 = 1.24%
- valid BUY rows: 354
- clean BUY FSC coverage: 6 / 354 = 1.69%
- decision miss `FSC_NO_RETAINED_RECIPIENT_HISTORY`: 13942
- BUY miss `FSC_NO_RETAINED_RECIPIENT_HISTORY`: 5677
- R26 had global recipient evictions and higher cap pressure.

R26B:

- lookup hit rate remained 5.28%, below expected >= 15%
- clean decision FSC coverage was 0.66%, below expected >= 10%
- clean BUY FSC coverage was 2.94%, below expected >= 10%
- global evictions were fixed mechanically: `0`
- per-recipient overflows dropped but remained non-zero: `113379`
- hot path stayed healthy, but coverage did not materially improve.

## Fail-Closed Behavior

FSC remained fail-closed as a policy signal:

- degraded/unavailable FSC did not become a policy approval
- `shadow_fsc_v2_policy_signal` stayed false on degraded/unavailable rows
- FSC was not used as a BUY validation signal
- no Gatekeeper policy, execution, send path, veto, or score behavior was changed

## Recovery Behavior

R26B did not show a provider hiccup or reconnect storm. The runtime stayed connected in watchdog logs with `reconnects=0`. Final disconnect/channel-closed messages are attributable to controlled shutdown.

## Why FSC Policy Signal Is Not Ready

FSC is not ready for policy use because:

- lookup hit rate stayed at 5.28%, not near the expected >= 15%
- clean decision coverage stayed below 1%
- clean BUY coverage stayed below 3%
- `FSC_NO_RETAINED_RECIPIENT_HISTORY` still dominated both decision and BUY rows
- increased global/per-recipient caps removed cap pressure but did not improve attribution coverage
- raw funding transfer artifacts were not persisted, so offline buyer-level attribution cannot be proven from durable evidence

## Disk And Log Size Warning

The run consumed roughly:

- rollout logs: 1020M
- reports: 487M
- capture: 299M
- root filesystem final free space: 16G, 90% used

Do not leave future canaries running without explicit duration/disk guards.

## Final Verdict

```text
R26B_RETENTION_CAPACITY_DELTA_MECHANICALLY_OK
FSC_COVERAGE_NOT_FIXED_BY_RETENTION_DELTA
NEXT_BOTTLENECK_ATTRIBUTION_WINDOW_OR_LOOKUP_SEMANTICS
```

