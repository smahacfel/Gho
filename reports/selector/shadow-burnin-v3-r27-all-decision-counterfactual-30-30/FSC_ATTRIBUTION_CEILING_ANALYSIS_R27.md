# FSC Attribution Ceiling Analysis R27

## Purpose

- purpose: offline FSC attribution coverage ceiling analysis for R27
- not BUY validation
- not Gatekeeper policy use
- policy/execution/send path changes: none
- new run started: no

## Input Artifacts

- funding_events_v1: 413579
- system_transfers_raw_v1: 413579
- lookup_candidates: 2370
- gatekeeper_v2_decisions: 440
- gatekeeper_v2_buys: 63
- audit_csv_rows: 2370
- dedup_pool_buy_events: 16028
- buy amount join source counts: signature_wallet_pool=2370
- lookup rows missing buy_lamports for relative threshold simulation: 0

## Method

- lookup identity: `fsc_lookup_candidates_v1.selected_lookup_wallet` / `lookup_wallet`
- funding source: `funding_events_v1.jsonl` indexed by `recipient_wallet`
- buy amount source: deduplicated `datasets/events/* PoolTransaction.payload.sol_amount_lamports` joined by signature/wallet/pool where possible
- row hit criterion: any inbound transfer to lookup_wallet before decision_ts_ms inside lookback window with `lamports >= min_abs_attribution_lamports` and `lamports / buy_lamports >= min_rel_to_buy`
- current hit means runtime sidecar `lookup_result == hit`
- sparse after 60m means no raw inbound transfer to any lookup wallet for that decision within 60m in available R27 full-chain evidence

## Row-Level Coverage

- lookup_rows total: 2370
- current runtime hits: 96 (4.05%)

### Raw inbound by lookback before thresholds

| lookback | rows with inbound | coverage |
|---|---:|---:|
| 5m | 419 | 17.68% |
| 15m | 618 | 26.08% |
| 30m | 634 | 26.75% |
| 60m | 634 | 26.75% |

### Current-threshold attribution by lookback

| lookback | abs | rel | hits | coverage |
|---|---:|---:|---:|---:|
| 5m | 10000000 | 0.20 | 96 | 4.05% |
| 15m | 10000000 | 0.20 | 139 | 5.86% |
| 30m | 10000000 | 0.20 | 145 | 6.12% |
| 60m | 10000000 | 0.20 | 145 | 6.12% |

### Threshold/lookback grid row-level

| lookback | abs | rel | hits | coverage | unlocked_from_current |
|---|---:|---:|---:|---:|---:|
| 5m | 10000000 | 0.2 | 96 | 4.05% | 0 |
| 5m | 10000000 | 0.1 | 108 | 4.56% | 12 |
| 5m | 10000000 | 0.05 | 117 | 4.94% | 21 |
| 5m | 5000000 | 0.2 | 96 | 4.05% | 0 |
| 5m | 5000000 | 0.1 | 109 | 4.60% | 13 |
| 5m | 5000000 | 0.05 | 118 | 4.98% | 22 |
| 5m | 2000000 | 0.2 | 96 | 4.05% | 0 |
| 5m | 2000000 | 0.1 | 109 | 4.60% | 13 |
| 5m | 2000000 | 0.05 | 118 | 4.98% | 22 |
| 5m | 1000000 | 0.2 | 96 | 4.05% | 0 |
| 5m | 1000000 | 0.1 | 109 | 4.60% | 13 |
| 5m | 1000000 | 0.05 | 118 | 4.98% | 22 |
| 15m | 10000000 | 0.2 | 139 | 5.86% | 43 |
| 15m | 10000000 | 0.1 | 150 | 6.33% | 54 |
| 15m | 10000000 | 0.05 | 162 | 6.84% | 66 |
| 15m | 5000000 | 0.2 | 140 | 5.91% | 44 |
| 15m | 5000000 | 0.1 | 151 | 6.37% | 55 |
| 15m | 5000000 | 0.05 | 163 | 6.88% | 67 |
| 15m | 2000000 | 0.2 | 140 | 5.91% | 44 |
| 15m | 2000000 | 0.1 | 152 | 6.41% | 56 |
| 15m | 2000000 | 0.05 | 164 | 6.92% | 68 |
| 15m | 1000000 | 0.2 | 140 | 5.91% | 44 |
| 15m | 1000000 | 0.1 | 152 | 6.41% | 56 |
| 15m | 1000000 | 0.05 | 164 | 6.92% | 68 |
| 30m | 10000000 | 0.2 | 145 | 6.12% | 49 |
| 30m | 10000000 | 0.1 | 157 | 6.62% | 61 |
| 30m | 10000000 | 0.05 | 169 | 7.13% | 73 |
| 30m | 5000000 | 0.2 | 146 | 6.16% | 50 |
| 30m | 5000000 | 0.1 | 158 | 6.67% | 62 |
| 30m | 5000000 | 0.05 | 170 | 7.17% | 74 |
| 30m | 2000000 | 0.2 | 146 | 6.16% | 50 |
| 30m | 2000000 | 0.1 | 159 | 6.71% | 63 |
| 30m | 2000000 | 0.05 | 171 | 7.22% | 75 |
| 30m | 1000000 | 0.2 | 146 | 6.16% | 50 |
| 30m | 1000000 | 0.1 | 159 | 6.71% | 63 |
| 30m | 1000000 | 0.05 | 171 | 7.22% | 75 |
| 60m | 10000000 | 0.2 | 145 | 6.12% | 49 |
| 60m | 10000000 | 0.1 | 157 | 6.62% | 61 |
| 60m | 10000000 | 0.05 | 169 | 7.13% | 73 |
| 60m | 5000000 | 0.2 | 146 | 6.16% | 50 |
| 60m | 5000000 | 0.1 | 158 | 6.67% | 62 |
| 60m | 5000000 | 0.05 | 170 | 7.17% | 74 |
| 60m | 2000000 | 0.2 | 146 | 6.16% | 50 |
| 60m | 2000000 | 0.1 | 159 | 6.71% | 63 |
| 60m | 2000000 | 0.05 | 171 | 7.22% | 75 |
| 60m | 1000000 | 0.2 | 146 | 6.16% | 50 |
| 60m | 1000000 | 0.1 | 159 | 6.71% | 63 |
| 60m | 1000000 | 0.05 | 171 | 7.22% | 75 |

### Unique Decision-Level Coverage

- total: 329
- current attribution hit: 77 (23.40%)
- base recomputed hit @5m abs=10M rel=0.20: 77 (23.40%)
- unlocked by threshold relax @5m abs=1M rel=0.05: 9
- unlocked by longer lookback @60m abs=10M rel=0.20: 29
- combined ceiling @60m abs=1M rel=0.05: 111 (33.74%)
- combined unlocked from current: 34
- raw inbound exists within 60m: 203 (61.70%)
- still sparse after 60m: 126 (38.30%)

### BUY Decision-Level Coverage

- total: 63
- current attribution hit: 19 (30.16%)
- base recomputed hit @5m abs=10M rel=0.20: 19 (30.16%)
- unlocked by threshold relax @5m abs=1M rel=0.05: 4
- unlocked by longer lookback @60m abs=10M rel=0.20: 9
- combined ceiling @60m abs=1M rel=0.05: 31 (49.21%)
- combined unlocked from current: 12
- raw inbound exists within 60m: 54 (85.71%)
- still sparse after 60m: 9 (14.29%)

## Quality Check: New Hits From Threshold Relax

- definition: rows not current hit, newly qualifying at 5m with abs=1M and rel=0.05
- new_hit_rows: 22
- median funding lamports: 61995442
- p10/p50/p90 funding lamports: 19882400 / 61995442 / 177655731
- rel_to_buy p10/p50/p90: 0.0622 / 0.1248 / 0.1749
- source_wallets: 10
- source HHI: 0.1942
- top1 source share: 36.36%
- top3 source share: 63.64%
- top sources: [('AxiomRXZAq1Jgjj9pHmNqVP7Lhu67wLXZJZbaK87TTSk', 8), ('3TJsN5VaESVeSGFHWpk4aJgA89UiHoQN2Y9MyBXHUiMb', 4), ('2AQdpHJ2JpcEgPiATUXjQxA8QmafFegfQwSLWSprPicm', 2), ('DGrdfmnCEQTbV7B7BtLptmDeqoB1YReFYYgMp5fqnyRb', 2), ('DwEbivjqmMEqCsnJp8vAdU9c6Td5wpygytDespm39Jrr', 1)]
- interpretation: source-concentrated enough to inspect, not purely random dust

## Interpretation

- Threshold relax adds 9 unique decisions and 4 BUY decisions at current 5m lookback.
- Longer lookback at current thresholds adds 29 unique decisions and 9 BUY decisions by 60m.
- Combined 60m + relaxed thresholds reaches 111/329 unique decisions and 31/63 BUY decisions.
- Still sparse after 60m remains 126/329 unique decisions and 9/63 BUY decisions.

## Final Verdict

FSC_LOOKBACK_WINDOW_CANARY_RECOMMENDED

## Non-Goals Preserved

- no Gatekeeper policy change
- no execution change
- no send path change
- no FSC veto/score proposal
- no R26/R27/R28 run started
