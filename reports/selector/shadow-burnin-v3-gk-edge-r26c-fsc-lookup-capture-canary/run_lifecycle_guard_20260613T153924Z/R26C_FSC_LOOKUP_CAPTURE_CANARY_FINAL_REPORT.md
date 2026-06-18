# R26C FSC Lookup Capture Canary Final Report

## Purpose

- purpose: FSC lookup-capture/evidence canary only
- not BUY validation
- not FSC coverage fix
- not policy test
- config path: `configs/rollout/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary.toml`
- launcher: `scripts/start_selector_lifecycle_run.py`
- tmux session: `selector_r26c_fsc_lookup_capture_canary`
- runtime dir: `reports/selector/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary/run_lifecycle_guard_20260613T153924Z`
- policy/execution/send path changes: none in this run

## Process State

- launcher status: `PASS`
- launcher claim: `SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF`
- launcher run_state: `RUN_LEFT_RUNNING_AFTER_LIFECYCLE_PROOF`
- lifecycle proof: `PASS`
- `legacy_buy_executable_rows`: 3
- `execution_feasibility_status_counts.executable`: 3
- tmux stopped at check time: yes, no tmux server
- stale `ghost-launcher` process: none
- disk after run: `/dev/sda1 150G 134G 11G 94% /`

## Runtime Health

- primary ingest status: active during run
- observed primary source: `grpc_global_stream`
- `ResourceExhausted`: 0
- `DataLoss`: 0
- `h2 protocol error`: 0
- `protocol error`: 0
- reconnect storm: not observed
- `reconnects=[1-9]`: 0
- Rust panic/crash markers: not observed
- runtime log size: about 189 MiB

Notes:

- Runtime contains shadow post-buy `PANIC SELL`/`ERROR` lines from WHF/SignalRouter. These are shadow post-buy signals, not Rust panics and not evidence of primary ingest failure.

## Durable Capture Acceptance

| Artifact | Rows | Bytes / Size | Status |
|---|---:|---:|---|
| `logs/nln_capture/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary/funding_events_v1.jsonl` | 0 | 0 | FAIL |
| `logs/nln_capture/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary/system_transfers_raw_v1.jsonl` | 0 | 0 | FAIL |
| `logs/rollout/.../v2.5/v25_shadow/.../fsc_lookup_candidates_v1.jsonl` | 4,829 | 8,082,913 | PASS |
| `logs/rollout/.../v2.2/legacy_live/.../fsc_lookup_candidates_v1.jsonl` | 4,829 | 7,972,137 | PASS |
| `logs/rollout/.../v2.5/v25_shadow/.../gatekeeper_v2_decisions.jsonl` | 850 | 70,879,672 | PASS |
| `logs/rollout/.../v2.2/legacy_live/.../gatekeeper_v2_decisions.jsonl` | 850 | 70,894,433 | PASS |
| `logs/rollout/.../v2.2/legacy_live/.../gatekeeper_v2_buys.jsonl` | 128 | 13,994,345 | PASS |
| `FSC_ATTRIBUTION_LOOKUP_AUDIT.md` | generated | non-empty | PASS |
| `FSC_ATTRIBUTION_LOOKUP_AUDIT.csv` | 9,659 lines | non-empty | PASS |

## Lookup Candidate Semantics

R26C did capture lookup semantics.

V2.5 lookup candidates:

- rows: 4,829
- `lookup_wallet_source=owner_token_delta_positive`: 4,638
- `lookup_wallet_source=signer_fallback`: 191
- `fallback_used=false`: 4,638
- `fallback_used=true`: 191
- `lookup_result=hit`: 42
- `lookup_result=miss`: 4,787
- selected lookup wallets present in sidecar: yes

BUY rows:

- BUY rows: 128
- BUY rows with nested `funding_source_diagnostics.lookup_diagnostics`: 128
- nested BUY lookup diagnostic rows: 2,193
- nested BUY `lookup_wallet` non-null rows: 2,193
- nested BUY `lookup_wallet_source=owner_token_delta_positive`: 2,181
- nested BUY `lookup_wallet_source=signer_fallback`: 12
- top-level BUY `selected_lookup_wallet` / `lookup_wallet`: absent

Interpretation:

- Sidecar and nested BUY diagnostics are enough to see which wallet was used for lookup.
- Lookup semantics appear to prefer buyer/user wallet from positive owner token delta, with signer fallback only when needed.
- There is no evidence in this run that lookup is accidentally using mint/pool/creator as the primary key.
- However, if the acceptance requires top-level BUY `lookup_wallet` fields rather than nested diagnostics, that part is only partially satisfied.

## Transfer Capture Failure

NLN capture manifest:

- `active_raw_grpc_streams`: 1
- `active_program_streams`: 1
- `active_enhanced_streams`: 1
- `active_program_streams_plus_enhanced_streams`: 2
- active topics:
  - `prod.rpc.solana.system.transfers` as `system_transfers`
  - `solana.pump_fun.buy` as `pumpfun_buy`
- `expected_missing_artifacts`: `[]`
- `has_transfers_topic`: false in runtime `ListTopics`
- `missing_selected_topics`: `["prod.rpc.solana.system.transfers"]`

Runtime evidence:

- FSC lane start observed: `started_topics=["prod.rpc.solana.system.transfers", "solana.pump_fun.buy"]`
- artifact writer started: yes
- `solana.pump_fun.buy` first message received: yes
- `prod.rpc.solana.system.transfers` subscribe failed: yes
- system transfer topic exited with:
  - `native_transfer_count=0`
  - `non_native_transfer_count=0`
  - `decode_error_count=1`
  - error: `NLN Subscribe request failed`

Interpretation:

- Pump.fun buy program-stream capture is alive.
- System transfer topic is not alive for this provider/topic selection.
- Because transfer rows never arrived, `funding_events_v1.jsonl` and `system_transfers_raw_v1.jsonl` remained empty.
- This run cannot validate `source_wallet` / `recipient_wallet` / `lamports` serialization from live transfer input because there was no transfer input.

## FSC Decision Coverage

Decision rows, v2.5:

- total decisions: 850
- FSC status `clean`: 1
- FSC status `degraded`: 119
- FSC status `unavailable`: 730
- `shadow_fsc_v2_policy_signal=true`: 1
- `max_funding_source_concentration` non-null: 850

BUY rows:

- total BUY rows: 128
- FSC status `clean`: 1
- FSC status `degraded`: 19
- FSC status `unavailable`: 108
- `shadow_fsc_v2_policy_signal=true`: 1
- `max_funding_source_concentration` non-null: 128

This is not BUY validation and not FSC policy readiness.

## Offline Audit

Outputs:

- `reports/selector/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary/run_lifecycle_guard_20260613T153924Z/FSC_ATTRIBUTION_LOOKUP_AUDIT.md`
- `reports/selector/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary/run_lifecycle_guard_20260613T153924Z/FSC_ATTRIBUTION_LOOKUP_AUDIT.csv`

Audit summary:

- `funding_events_rows`: 0
- `lookup_rows`: 9,658
- `audited_rows`: 9,658
- `found_5m`: 0
- `found_15m`: 0
- `found_30m`: 0
- `found_60m`: 0
- dominant diagnosed bottleneck: `DIRECT_FUNDING_NOT_OBSERVED_60M`

Important caveat:

- The audit could compute 5/15/30/60 minute joins mechanically, but all joins are vacuous because the funding-events input is empty.
- Therefore this run does not prove direct funding is inherently sparse.
- It proves live transfer capture is still missing for this selected NLN topic.

## Answer To The Attribution Question

For R26C, the dominant `FSC_NO_RETAINED_RECIPIENT_HISTORY`/unavailable behavior cannot yet be attributed to:

- too short lookback window,
- too strict abs/relative thresholds,
- same-slot ordering,
- wrong lookup wallet,
- or inherently sparse direct funding graph.

The immediate blocker is:

- transfer capture is still empty,
- `prod.rpc.solana.system.transfers` was not available/listed by NLN during this run,
- subscription to that topic failed,
- no raw system transfer rows reached the durable artifact writer.

## Final Verdict

`FSC_TRANSFER_CAPTURE_STILL_EMPTY`

Equivalent expanded verdict:

- `LIFECYCLE_PROOF_PASS`
- `LOOKUP_WALLET_CAPTURE_PASS`
- `OFFLINE_AUDIT_MECHANICALLY_PASS`
- `SYSTEM_TRANSFER_TOPIC_UNAVAILABLE`
- `FUNDING_EVENTS_EMPTY`
- `FSC_ATTRIBUTION_AUTOPSY_STILL_BLOCKED`

