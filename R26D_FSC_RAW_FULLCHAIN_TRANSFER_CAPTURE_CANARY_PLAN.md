# R26D FSC Raw Full-Chain Transfer Capture Canary Plan

## Purpose

- Validate durable native SOL transfer capture from raw Yellowstone full-chain funding lane.
- This is not BUY validation.
- This is not an FSC coverage fix validation.
- This is not a policy test.
- This must not change Gatekeeper policy, execution, send path, FSC veto, score, caps, windows, or thresholds.

## Source Contract

- Primary transfer source: `grpc_funding_lane_full_chain` raw `ALL_TRANSACTIONS`.
- Optional transfer source: `prod.rpc.solana.system.transfers`, only if provider exposes that topic.
- Missing decoded program-stream topic must not make `system_transfers_raw_v1.jsonl` or `funding_events_v1.jsonl` empty when raw full-chain lane emits native SOL transfers.

## Config

- Candidate config: `configs/rollout/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary.toml` copied to a new R26D profile before launch.
- Required mode: `funding_lane_mode = "full_chain"`.
- Required launcher: `scripts/start_selector_lifecycle_run.py`.
- Required guard: lifecycle proof must pass before the run is accepted.
- Duration: short controlled canary, 10-15 minutes unless warmup needs a small extension.
- No R26D launch without separate explicit approval.

## Expected Evidence Path

```text
grpc_funding_lane_full_chain raw ALL_TRANSACTIONS
-> Seer raw transaction event
-> native SOL SystemProgram transfer decode
-> SeerEvent::FundingTransfer(full_chain_coverage=true)
-> launcher IPC bridge
-> system_transfers_raw_v1.jsonl
-> funding_events_v1.jsonl
-> GhostEvent::FundingTransferObserved
-> FundingSourceIndex.observe_transfer(recipient_wallet)
```

## Metrics To Snapshot

- `fsc_transfer_source_available{source="grpc_funding_lane_full_chain"}`
- `fsc_transfer_source_available{source="program_stream_system_transfers"}`
- `fsc_raw_fullchain_transactions_seen_total`
- `fsc_raw_fullchain_system_transfer_decode_attempts_total`
- `fsc_raw_fullchain_system_transfers_decoded_total`
- `fsc_system_transfers_raw_written_total`
- `fsc_funding_events_written_total`
- `fsc_transfer_decode_failures_total{reason}`
- `fsc_lookup_candidates_v1.jsonl` row count
- `funding_events_v1.jsonl` row count
- `system_transfers_raw_v1.jsonl` row count
- primary ingest health, ResourceExhausted, reconnect/DataLoss/h2/protocol errors

## Acceptance

PASS if:

- `grpc_funding_lane_full_chain` events are observed.
- `fsc_raw_fullchain_transactions_seen_total > 0`.
- `fsc_raw_fullchain_system_transfers_decoded_total > 0`.
- `system_transfers_raw_v1.jsonl > 0`.
- `funding_events_v1.jsonl > 0`.
- `fsc_lookup_candidates_v1.jsonl > 0`.
- Offline audit can join `lookup_wallet -> inbound transfer`.
- No ResourceExhausted.
- No reconnect storm.
- Policy, execution and send path are unchanged.

FAIL if:

- Raw full-chain events are observed, but decoded transfers stay at 0.
- Decoded transfers are observed, but artifact rows stay at 0.
- Artifacts depend only on missing `prod.rpc.solana.system.transfers`.
- `funding_events_v1.jsonl` remains 0.

## Stop And Report

- Stop after the bounded canary window.
- Capture `df -h`, artifact row counts, metrics snapshot, runtime log error scan, and process/tmux state.
- Run `scripts/fsc_attribution_lookup_audit.py` after stop if artifacts are non-empty.
- Write final R26D report before proposing any R26E/R27.

## Possible Verdicts

- `FSC_RAW_FULLCHAIN_TRANSFER_CAPTURE_READY_FOR_CANARY`
- `FSC_RAW_TRANSACTION_DECODE_BUG_FOUND`
- `FSC_TRANSFER_ARTIFACT_WRITER_BUG_FOUND`
- `FSC_PROVIDER_RAW_FULLCHAIN_LACKS_TRANSFER_DATA`
- `FSC_CAPTURE_STILL_BLOCKED_BY_SOURCE_CONTRACT`
