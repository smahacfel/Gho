# PR1B baseline receipt — ingest hot path

Date: 2026-07-24  
Repository: `/root/Gho_ingest` (`smahacfel/Gho`)  
Clean parent: `ea7d31a228f8db0b7ed0779dea70b696895e66c2`  
Branch used for the harness commit: `agent/ingest-single-pass-pr1b-20260724`

## Repository provenance

- `origin/main` was fetched before the branch was created.
- `origin/main` resolved to `ea7d31a228f8db0b7ed0779dea70b696895e66c2`.
- The commit is the merge commit of PR #82 and is an ancestor of the PR1B branch.
- The existing `/root/Gho_ingest` checkout was used. No other clone, worktree reset, cleanup, or destructive command was used.

## Measured active live call graph

```text
Yellowstone SubscribeUpdate (already decoded by tonic/prost)
  -> stream_loop
  -> route_update
       encode_proto(SubscribeUpdateTransaction)                [encode #1]
  -> PumpEvent::Transaction { raw: Vec<u8> }
  -> DualLaneChannel::try_send
       fast queue -> overflow queue -> blocking overflow.send
  -> pump_event_to_geyser_event
  -> decode_tx_to_geyser_event                                 [decode #1]
  -> tx_update_to_geyser_event
  -> GeyserEvent::Transaction { mpcf_payload_bytes: raw }
  -> Seer::run bounded event worker
  -> Seer::process_event
       append_raw_tx_to_wal
         raw.clone()
         wal.append_with_clock                                 [synchronous]
       raw_pumpfun_instruction_evidence_rows
         Base58/String/JSON construction                       [synchronous]
       BinaryParser::parse_initialize_pool
         parse_pump_events
         PumpParser::parse_transaction_raw                     [decode #2, scan #1]
       BinaryParser::parse_trades
         parse_pump_events
         PumpParser::parse_transaction_raw                     [decode #3, scan #2]
       IpcSender::send_*().await
         BackpressurePolicy::Block                             [worker may wait]
```

For one ordinary live transaction the application therefore performed:

- one prost encode after Yellowstone had already returned a decoded message;
- three prost decodes: one in normalization and one in each of the two parser calls;
- two complete outer/inner instruction-tree scans;
- one ownership move of the encoded `Vec<u8>` through the transport;
- one full raw-buffer clone for synchronous WAL capture when WAL was enabled;
- additional whole-event/raw clones on selected diagnostic/enhanced paths.

For `create + initial buy`, the same transaction was also scanned twice: once to obtain pool initialization and once to obtain trades.

The receive loop stopped draining when both ingress lanes were full because `DualLaneChannel::try_send` fell back to blocking `overflow.send`. Separately, the bounded `event_worker_concurrency` stopped `Seer::run` from draining the event stream after all event workers waited on synchronous WAL work or blocking IPC capacity.

## Deterministic workload

The ignored unit harness `hot_path_harness::pr1b_hot_path_harness` uses protobuf fixtures constructed in-process and covers:

- ordinary Pump.fun buy;
- ordinary Pump.fun sell;
- create plus initial buy;
- two Pump mutations in one signature;
- PumpSwap trade in inner instructions;
- account update;
- a 2,048-event ingress burst;
- a deliberately slow WAL sink;
- a deliberately slow IPC consumer;
- a capacity-two queue saturation episode.

The business summary is canonicalized and hashed. Baseline digest:

```text
062d36ab094fb470909fd9836318fee85d89dbed8f1a9a86080041f20a399ee2
```

This digest is the differential parity gate for the final PR1B implementation.

## Release-profile baseline result

Command:

```bash
cargo test --release -p seer --lib pr1b_hot_path_harness -- \
  --ignored --nocapture --test-threads=1
```

Result: PASS (`1 passed`, `460 filtered out`).

| Metric | Baseline |
|---|---:|
| Throughput | 2,117.317 transactions/s |
| receive-to-normalize p50 | 22,976 ns |
| receive-to-normalize p95 | 38,558 ns |
| receive-to-normalize p99 | 53,377 ns |
| normalize-to-parsed-bundle p50 | 448,334 ns |
| normalize-to-parsed-bundle p95 | 577,705 ns |
| normalize-to-parsed-bundle p99 | 610,701 ns |
| Burst input | 2,048 events |
| Queue high-water | 2,048 events |
| Oldest queued event age | 4,104,891 ns |
| Steady-state RSS | 71,764 KiB |
| CPU time | unavailable from the deterministic harness |
| Application prost encode, five transactions | 5 |
| Normalizer prost decode, five transactions | 5 |
| Parser prost decode, five transactions | 10 |
| Full instruction-tree scans, five transactions | 10 |
| WAL append calls / blocking waits | 2 / 2 |
| Slow WAL elapsed | 13,278,838 ns |
| IPC blocking waits | 1 |
| Slow IPC elapsed | 11,429,833 ns |
| Capacity-two ingress blocked before drain | yes |
| Capacity-two blocking wait | 10,413,925 ns |
| Silent drops reported | 0 |

The throughput/latency/RSS values are comparative harness measurements on this host, not a production capacity claim. The measured release throughput multiplied by a 250 ms allowed burst is approximately 530 events. A power-of-two capacity of 1,024 events is therefore the initial measured ingress bound (about 483 ms at measured harness throughput), subject to the final after-measurement and bounded-memory test.

## Debug-profile diagnostic result

The same harness passed in the debug profile. It produced the same business digest and the same operation counts:

```text
prost encode=5
normalizer prost decode=5
parser prost decode=10
full scans=10
```

Debug throughput was 602.834 transactions/s, normalize-to-bundle p99 was 2,541,417 ns, and RSS was 84,292 KiB. These values are retained only to make test-profile regressions visible.

## Baseline verification and existing failure signature

Passed:

```text
cargo fmt --all --check
git diff --check
cargo test -p ghost-core ingest_integrity -- --nocapture
```

The targeted release harness passed as recorded above.

The unfiltered baseline command below did not finish within 120 seconds:

```text
timeout 120s cargo test -p seer --lib -- --test-threads=1
```

Before timeout, it reported pre-existing failures in PumpPortal/Seer tests, including:

```text
pumpportal_connection::tests::test_buy_emits_only_tx
pumpportal_connection::tests::test_create_emits_both_events
pumpportal_connection::tests::test_create_pool_detected_before_tx
pumpportal_connection::tests::test_create_to_pool_transaction
pumpportal_connection::tests::test_price_derived_from_reserves
pumpportal_connection::tests::test_price_none_when_no_reserves
pumpportal_connection::tests::test_price_none_when_zero_tokens
pumpportal_connection::tests::test_pumpportal_no_slot
pumpportal_connection::tests::test_sell_emits_only_tx
pumpportal_connection::tests::test_sol_amount_to_lamports_precision
tests::test_create_sets_curve_mapping
tests::test_pumpswap_initialize_pool_known_mint_seeds_continuity_without_pool_detected
tests::test_pumpswap_initialize_pool_without_known_mint_is_suppressed
tests::test_session_start_slot_rejects_old_pools
```

Those failures are not masked and are outside the PR1B ingest-boundary scope. The final receipt must compare this signature and separately pass the new PR1B gates.

## B0 scope statement

The B0 changes add only `cfg(test)` counters, deterministic fixtures, the ignored harness, and this receipt. `ParsedTransactionBundle` is introduced only as a compatibility wrapper around both legacy parser calls so the baseline can count existing work; no active runtime call site is changed in B0.
