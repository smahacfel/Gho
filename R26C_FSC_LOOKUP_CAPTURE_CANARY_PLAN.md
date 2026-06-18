# R26C FSC Lookup Capture Canary Plan

## Purpose

R26C verifies durable FSC attribution evidence capture only.

It is not BUY validation.
It is not a coverage fix.
It must not change Gatekeeper policy, execution, send path, FSC veto, or FSC score.

## Background

R26B ended with:

- `FSC_COVERAGE_NOT_FIXED_BY_RETENTION_DELTA`
- global cap was not the main blocker
- per-recipient overflow dropped
- hot path stayed healthy
- lookup hit rate and clean coverage did not materially improve

The blocker for the next autopsy was missing durable evidence:

- `funding_events_v1.jsonl = 0`
- `system_transfers_raw_v1.jsonl = 0`
- decisions did not persist the buyer/lookup wallet list

## Canary Objective

Confirm that the runtime produces enough durable evidence to explain FSC misses offline:

- normalized full-chain funding events
- raw system transfer rows
- per-decision FSC lookup candidates
- per-decision FSC lookup diagnostics
- offline 5/15/30/60 minute join output

## Required Artifacts

- `funding_events_v1.jsonl`
- `system_transfers_raw_v1.jsonl`
- `fsc_lookup_candidates_v1.jsonl`
- `FSC_ATTRIBUTION_LOOKUP_AUDIT.md`
- `FSC_ATTRIBUTION_LOOKUP_AUDIT.csv`

## Acceptance Criteria

- `funding_events_v1.jsonl > 0`
- `system_transfers_raw_v1.jsonl > 0`
- `fsc_lookup_candidates_v1.jsonl > 0`
- BUY rows have `lookup_wallet` through sidecar diagnostics
- offline audit computes 5/15/30/60 minute joins
- no Gatekeeper policy changes
- no execution changes
- no send path changes
- no FSC veto/score activation
- degraded FSC evidence is not marked clean

## Expected Evidence Fields

Each `funding_events_v1` row should include:

- `slot`
- `signature`
- `block_time` or `ts_ms`
- `source_wallet`
- `recipient_wallet`
- `lamports`
- `transfer_kind`
- `source_label = grpc_funding_lane_full_chain`
- `parser_status`

Each `fsc_lookup_candidates_v1` row should include:

- `decision_id` or `ab_record_id`
- `mint`
- `pool_id`
- `decision_ts_ms`
- `slot`
- `signature`
- `candidate_wallets`
- `selected_lookup_wallet`
- `lookup_wallet_source`
- `fallback_used`
- `lookup_result`
- `history_entries_found`
- `latest_funding_age_ms`
- `matched_source_wallets_count`
- `matched_total_lamports`
- `miss_reason`
- `diagnostic_miss_reason`

## Snapshot Commands

Before stopping the canary, collect:

- `df -h`
- runtime/log sizes
- counts for `funding_events_v1.jsonl`
- counts for `system_transfers_raw_v1.jsonl`
- counts for `fsc_lookup_candidates_v1.jsonl`
- count of BUY rows with lookup wallet
- FSC evidence status counts
- top `miss_reason`
- top `diagnostic_miss_reason`
- ResourceExhausted/reconnect/DataLoss/h2 errors
- primary ingest status

## Offline Audit

Run `scripts/fsc_attribution_lookup_audit.py` with:

- decisions JSONL paths
- BUY JSONL paths
- `funding_events_v1.jsonl`
- `fsc_lookup_candidates_v1.jsonl`

Expected output:

- `FSC_ATTRIBUTION_LOOKUP_AUDIT.md`
- `FSC_ATTRIBUTION_LOOKUP_AUDIT.csv`

The CSV must include:

- `decision_id`
- `lookup_wallet`
- `decision_ts_ms`
- `found_5m`
- `found_15m`
- `found_30m`
- `found_60m`
- `latest_funding_age_ms`
- `funding_amount_lamports`
- `source_wallet`
- `miss_reason`
- `diagnosed_bottleneck`

## Final Verdict Options

Use one of:

- `FSC_LOOKUP_CAPTURE_READY_FOR_CANARY`
- `FSC_LOOKUP_KEY_BUG_FOUND`
- `FSC_TRANSFER_CAPTURE_MISSING`
- `FSC_DECISION_LOOKUP_WALLETS_MISSING`
- `FSC_ATTRIBUTION_AUTOPSY_STILL_BLOCKED`

## Non-Goals

- Do not start R26C without explicit approval.
- Do not tune coverage.
- Do not change FSC policy usage.
- Do not add FSC as veto or score.
- Do not change execution.
- Do not change send path.
- Do not interpret R26C as BUY validation.
