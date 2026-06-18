# R27B FSC Lookback Window Canary Final Report

## Purpose

- purpose: R27B FSC lookback-window canary + coverage comparison only
- not BUY validation
- not Gatekeeper policy use
- policy/execution/send path changes: none
- FSC veto/score: disabled / unchanged
- changed test knob: `fsc_v2.lookback_window_s = 1800`
- unchanged thresholds: min_abs_store_lamports=1_000_000, min_abs_attribution_lamports=10_000_000, min_rel_to_buy=0.20
- unchanged caps: fsc_global_recipient_cap=50_000, fsc_per_recipient_cap=256

## Run Status

- launcher: `scripts/start_selector_lifecycle_run.py`
- tmux session after check: stopped; no active `ghost-launcher` process found
- run end mode: runtime `timeout 2700s`
- guard result: PASS (`SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF`)
- event canary: PASS
- lifecycle canary: PASS
- config path: `configs/rollout/shadow-burnin-v3-r27b-fsc-lookback-window-canary.toml`
- brain config path: `configs/rollout/ghost_brain_selector_dataset_sampler_r27b_fsc_lookback_1800.toml`
- coverage window became ready: `993145: 2026-06-13T23:08:48.832013Z  INFO ghost_launcher::oracle_runtime: FSC authoritative funding coverage gate updated gate_enabled=true stream_available=true warmup_ready=true coverage_window_ready=true authoritative_buy_gate_open=true coverage_window_remaining_ms=0 source="funding_transfer"`

## Artifact Health

| artifact | R27 | R27B |
|---|---:|---:|
| funding_events_v1.jsonl rows | 413579 | 841796 |
| system_transfers_raw_v1.jsonl rows | 413579 | 841796 |
| fsc_lookup_candidates_v1 rows | 2370 | 4870 |
| gatekeeper_v2_decisions rows | 440 | 718 |
| gatekeeper_v2_buys rows | 63 | 111 |

R27B artifact contract is satisfied: funding events, raw transfers, and lookup candidates are all non-empty.

## Coverage Comparison

| metric | R27 lookback=300s | R27B lookback=1800s |
|---|---:|---:|
| runtime lookup hits | 96 / 2370 (4.05%) | 400 / 4870 (8.21%) |
| unique decision coverage | 77 / 329 (23.40%) | 207 / 624 (33.17%) |
| BUY decision coverage | 19 / 63 (30.16%) | 47 / 111 (42.34%) |
| offline raw inbound 5m | 419 | 997 |
| offline raw inbound 15m | 618 | 1438 |
| offline raw inbound 30m | 634 | 1782 |
| offline raw inbound 60m | 634 | 1877 |

Interpretation: lookback 1800s produced a material coverage gain versus R27 baseline, while thresholds stayed unchanged.

## FSC Status Counts

| plane | status | R27 | R27B |
|---|---|---:|---:|
| decisions | clean | 4 | 14 |
| decisions | degraded | 436 | 704 |
| decisions | unavailable | 0 | 0 |
| BUY | clean | 2 | 1 |
| BUY | degraded | 61 | 110 |
| BUY | unavailable | 0 | 0 |

Clean FSC coverage improved on decision rows in absolute count, but BUY-level clean rows did not improve in this sample. Runtime attribution coverage is better, but FSC remains mostly degraded, so this is not policy-ready.

## Miss Reasons

Top R27B runtime lookup miss reasons:

- FSC_NO_RETAINED_RECIPIENT_HISTORY: 4158
- none: 400
- FSC_RELATIVE_FUNDING_TOO_SMALL: 126
- FSC_GLOBAL_RECIPIENT_EVICTED: 56
- FSC_SAME_SLOT_ORDERING_UNAVAILABLE: 47
- FSC_ABS_ATTRIBUTION_TOO_SMALL: 41
- FSC_LOW_ATTRIBUTION_CONFIDENCE: 41
- FSC_NO_PREBUY_TRANSFER_IN_WINDOW: 1

Top R27B diagnostic miss reasons:

- NO_INBOUND_TRANSFER_OBSERVED: 3088
- INBOUND_EXISTS_BUT_BELOW_ABS_STORE_THRESHOLD: 1070
- none: 400
- INBOUND_EXISTS_BUT_BELOW_REL_THRESHOLD: 126
- INBOUND_EXISTS_BUT_PRUNED_BY_WINDOW: 56
- SAME_SLOT_ORDERING: 47
- UNKNOWN: 42
- INBOUND_EXISTS_BUT_BELOW_ABS_ATTRIBUTION_THRESHOLD: 41

`FSC_NO_RETAINED_RECIPIENT_HISTORY` increased in absolute rows because R27B processed more rows, but its share dropped from 93.71% to 85.38%.

## Capacity / Retention Notes

- R27B reached the 1800s coverage window at `23:08:48Z` and continued until timeout around `23:23:48Z`.
- Last live pre-ready scrape showed index entries near cap: 48,572 / 50,000, estimated memory 4,787,256,320 bytes, global cap evictions 0 at that time, per-recipient overflows 61,900.
- Final Prometheus scrape is not available because the run had already stopped when checked.
- Final sidecar miss reasons include `FSC_GLOBAL_RECIPIENT_EVICTED=56` and diagnostic `INBOUND_EXISTS_BUT_PRUNED_BY_WINDOW=56`, so the 50k cap did start to matter late in R27B.
- Cap-implied estimated memory at 50,000 recipients with current estimator is about 4,928,000,000 bytes.

## Primary Ingest Health

- last watchdog: `2026-06-13T23:22:48.686556Z  INFO ghost_launcher::components::watchdog: WATCHDOG | grpc_state=CONNECTED reconnects=0 | age_grpc=2ms age_ipc=0ms age_bus=0ms age_gk=5437ms age_dec=5411ms age_buys=32429ms age_events=5437ms`
- ResourceExhausted: 0
- DataLoss: 0
- transport/protocol errors: 0
- reconnect storm: 0
- GOAWAY/goaway: 0
- shadow RPC 429 Too Many Requests: 2
- Gatekeeper BUY PATH FAILED rows: 5

Primary gRPC ingest remained connected with `reconnects=0` in the last watchdog sample. The 429s were shadow RPC simulate failures against Alchemy, not primary gRPC funding/global-stream degradation.

## Disk / Log Size

| area | R27 | R27B |
|---|---:|---:|
| reports | 203M | 442M |
| nln_capture | 1.2G | 2.4G |
| rollout logs | 450M | 915M |
| datasets/events | 53M | 122M |

Final disk: `/dev/sda1` 150G total, 66G used, 79G available, 46% used.

## Final Verdict

R27B_FSC_LOOKBACK_WINDOW_CANARY_PASS_COVERAGE_IMPROVED_NOT_POLICY_READY

Expanded verdict:

- WIRING_PASS
- RAW_TRANSFER_CAPTURE_PASS
- LOOKBACK_1800S_COVERAGE_GAIN_PASS
- PRIMARY_INGEST_HEALTH_PASS
- POLICY_SIGNAL_NOT_READY
- NEXT_CAPACITY_BOTTLENECK_OBSERVED_AT_50K_RECIPIENT_CAP

Do not promote FSC into policy/veto/score based on this canary. The next step, if approved separately, should account for the fact that 1800s lookback improves attribution but starts pressing against the 50k recipient cap.
