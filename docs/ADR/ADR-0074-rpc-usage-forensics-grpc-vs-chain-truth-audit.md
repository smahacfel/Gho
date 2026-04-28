# ADR-0074: RPC Usage Forensics — gRPC Ingest vs Chain-Truth Audit Load

**Date:** 2026-04-02
**Status:** Accepted
**Author:** Ghost Father

## Context

A forensic investigation was requested to explain unexpectedly high Solana RPC usage, especially large `getTransaction` volume and elevated account-read traffic, without making any runtime code changes.

The key ambiguity was whether Seer was actually using Yellowstone gRPC as the primary ingest plane, or whether the production system was effectively still RPC-driven despite the intended architecture.

The investigation used three evidence classes:
- production configuration in `config.toml`
- active source code paths in Seer, Trigger, and Launcher
- runtime logs under `/root/Gho/logs/`, especially coverage and audit artifacts

## Decision

The system is confirmed to be using Yellowstone gRPC as the primary ingest transport for live Seer intake.

The unexpectedly high RPC usage is instead explained by concurrent secondary RPC consumers, with the dominant confirmed source being chain-truth / coverage reconstruction that fetches signatures and then transactions over RPC.

Confirmed conclusions:
1. Seer is configured for gRPC primary ingest (`source_mode = "grpc"`).
2. Runtime logs confirm live gRPC ingest via `grpc_global_stream`.
3. Runtime logs also confirm active RPC-heavy coverage windows performing signature pagination and `getTransaction` fetches.
4. Therefore, the main explanation is not “Seer ignores gRPC”; it is “gRPC ingest is live, while audit/reconstruction/fallback planes still generate substantial RPC load in parallel”.
5. Trigger contains a canonical-state-to-RPC fallback implementation (`ShadowLedgerPriceOracle` in `revolver_price_feed.rs`), but deeper code-usage analysis indicates that this path is not part of the current launcher live-sell runtime. The sampled runtime uses `PostBuyRuntime` + direct Shadow Ledger reads for live exit, and sampled system logs showed no activation strings for the legacy polling worker / canonical-miss RPC oracle path.
6. Therefore, trigger-side `getAccountInfo` from that oracle fallback should currently be treated as a legacy / dormant code path unless separate runtime evidence proves activation in another execution lane.

## Architectural Impact

This decision separates two concerns that were previously easy to conflate:
- **Primary ingest plane:** Yellowstone gRPC (`seer::grpc_connection`, `grpc_global_stream`)
- **Verification / recovery / fallback plane:** RPC (`ghost-launcher` chain-truth coverage, Seer fallback/backfill paths)
- **Legacy trigger oracle plane:** dormant code that still contains RPC fallback semantics but does not appear to be wired into the current live launcher path.

This means RPC billing cannot be interpreted as direct proof that Seer is failing over globally. RPC volume must be attributed by subsystem and purpose.

Components implicated by this distinction:
- `off-chain/components/seer`
- `ghost-launcher/src/oracle_runtime.rs`
- `off-chain/components/trigger/src/revolver_price_feed.rs`
- coverage/audit log pipelines under `/root/Gho/logs/decisions.jsonl/`

## Risk Assessment

**Rating:** Medium

Regression / interpretation risks:
- Misattributing RPC costs to Seer transport can lead to the wrong remediation.
- Disabling audit/reconstruction blindly could reduce observability or hide ingest gaps.
- Assuming trigger-side `getAccountInfo` dominance from the legacy oracle path would overstate certainty; the current live launcher path appears to bypass that oracle entirely.

## Consequences

What becomes easier:
- Future optimization can target the real RPC-heavy subsystems first.
- Seer transport discussions can stop treating gRPC viability as the primary unknown.
- Coverage/audit traffic can be budgeted and rate-limited as a separate concern.

What becomes harder:
- Billing analysis must distinguish ingest traffic from observability/reconstruction traffic.
- “RPC high” is no longer a single-root-cause diagnosis; it requires per-plane attribution.

## Alternatives Considered

### Alternative 1: Conclude that Seer is not really using gRPC
Rejected because runtime evidence shows `grpc_global_stream` in received/emitted coverage audit logs and system coverage logs showing `grpc_rx > 0` with sampled `rpc_fallback_sigs=0` and `rpc_fallback_events=0`.

### Alternative 2: Conclude that Seer manual backfill is the primary RPC driver
Rejected as the primary explanation for the sampled production window because sampled Seer coverage logs show zero fallback share, while coverage decision logs show direct evidence of large RPC transaction-reconstruction windows.

### Alternative 3: Treat trigger-side `getAccountInfo` fallback as active in the current live runtime
Rejected because symbol-usage analysis shows the relevant oracle/polling types are not instantiated outside their own module/tests, while the active live path in `post_buy_runtime.rs` reads price directly from Shadow Ledger and uses ATA balance RPC only after `PostBuySubmitted`.

## Validation Steps

1. Confirm production config still sets Seer to gRPC primary ingest.
2. Inspect `seer_runtime_coverage_audit.jsonl` for `raw_received_by_source.grpc_global_stream` and matching emitted counters.
3. Inspect system coverage logs for Seer metrics including `grpc_rx`, `rpc_fallback_sigs`, and `rpc_fallback_events`.
4. Inspect coverage decision logs for RPC reconstruction fields such as:
   - `rpc_pool_signature_total_tx`
   - `rpc_sig_pages_fetched`
   - `rpc_latency_ms`
   - `rpc_fetch_error`
5. Inspect Trigger canonical oracle implementation and its symbol usages before attributing runtime load to it.
6. Confirm current live path in `ghost-launcher/src/components/post_buy_runtime.rs` (post-buy event -> live sell lifecycle -> direct Shadow Ledger reads).
7. If needed, add future per-method telemetry split by subsystem before making cost-cutting changes.
