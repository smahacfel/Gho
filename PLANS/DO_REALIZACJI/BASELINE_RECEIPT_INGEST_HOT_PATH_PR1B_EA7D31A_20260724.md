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

---

## Final PR1B receipt

Final branch: `agent/ingest-single-pass-pr1b-20260724`
Final parent: `ea7d31a228f8db0b7ed0779dea70b696895e66c2`
Harness profile: release, identical deterministic workload and command as B0.

### Root cause confirmed in code and measurement

The baseline backlog was self-generated before true overload handling became
relevant:

1. the already-decoded Yellowstone transaction was prost-encoded for internal
   transport and then decoded once by normalization and twice by the two parser
   entry points;
2. CREATE and TRADE parsing independently scanned the same outer and inner
   instruction tree;
3. the event worker performed physical WAL append and raw-buffer cloning;
4. raw evidence JSON/Base58/hash preparation happened before writer handoff;
5. critical IPC could await downstream capacity inside every event worker;
6. the two-lane ingress ended in a blocking `overflow.send`.

The final implementation removes that repeated work before defining overload
semantics. The unchanged business digest proves that the deterministic corpus
still emits the same canonical business result:

```text
062d36ab094fb470909fd9836318fee85d89dbed8f1a9a86080041f20a399ee2
```

### Before/after live call graph

```text
BEFORE
decoded SubscribeUpdateTransaction
  -> prost encode
  -> fast queue -> overflow queue -> blocking overflow.send
  -> prost decode in normalizer
  -> synchronous raw WAL clone + append
  -> evidence Base58/String/JSON/hash on event worker
  -> parse CREATE: prost decode + full outer/inner scan
  -> parse TRADE:  prost decode + full outer/inner scan
  -> IPC send().await with Block policy

AFTER
decoded SubscribeUpdateTransaction
  -> one bounded ingress FIFO / nonblocking try_send
  -> direct normalization from decoded fields
  -> optional shared capture after ingress
       capture disabled: no prost encode
       capture required: one prost encode, never decoded by live parser
  -> parse_transaction_bundle
       one outer/inner scan
       one dedupe/provenance/ordinal pass
       PoolDetected before all Trade events
  -> nonblocking bounded handoff
       WAL job -> one fixed writer -> physical append
       evidence Arc -> one fixed writer -> Base58/JSON/hash/file
       typed event -> one fixed IPC dispatcher -> downstream capacity wait
```

### Release-profile comparison

| Metric | Clean parent B0 | Final PR1B | Change |
|---|---:|---:|---:|
| Throughput | 2,117.317 events/s | 2,529.194 events/s | +19.453% |
| receive-to-normalize p50 | 22,976 ns | 20,662 ns | |
| receive-to-normalize p95 | 38,558 ns | 35,562 ns | |
| receive-to-normalize p99 | 53,377 ns | 44,751 ns | -16.161% |
| normalize-to-bundle p50 | 448,334 ns | 390,216 ns | |
| normalize-to-bundle p95 | 577,705 ns | 468,555 ns | |
| normalize-to-bundle p99 | 610,701 ns | 513,846 ns | -15.860% |
| Queue high-water | 2,048 | 1,024 | -50.000% |
| Oldest queued event age | 4,104,891 ns | 3,999,365 ns | |
| Steady-state RSS | 71,764 KiB | 24,140 KiB | -66.362% |
| CPU time | unavailable | unavailable | not claimed |
| Prost encode / 5 live tx | 5 | 0 with capture off; 5 with required capture | at most 1/tx |
| Normalizer prost decode / 5 live tx | 5 | 0 | -5 |
| Parser prost decode / 5 live tx | 10 | 0 | -10 |
| Full instruction scans / 5 live tx | 10 | 5 | -5 |
| Event-worker WAL blocking waits | 2 | 0 | removed |
| Parser-worker IPC blocking waits | 1 | 0 | removed |

These are comparative harness measurements on this host, not a production
capacity or losslessness claim.

### Queue model and bounded overload result

- Main ingress: one crossbeam bounded FIFO, capacity 1,024.
- Capacity derivation: 2,117.317 measured events/s × 250 ms = 529.329,
  rounded up to the next power of two.
- WAL: one bounded queue, capacity 1,024, one fixed OS writer thread.
- Raw evidence: one bounded queue, capacity 1,024, one fixed writer task.
- IPC egress: one bounded queue, configured from the existing IPC buffer size,
  one fixed dispatcher thread.
- No general overflow/spill queue and no per-event task spawning.

In the final capacity-two saturation case:

```text
receiver blocked: false
blocking wait: 0 ns
explicit missing events: 2
local gaps emitted: 1
silent drops reported as success: 0
gap id: HRXk4UWUX3dQpf6RwftCizfPHPuxSwEPNuPrUYHKdxhC
reason: ingress_queue_saturated
recovered: false
```

The 2,048-event burst filled the 1,024-event bound and accounted explicitly for
the remaining 1,024 events in the harness overload model. The segment becomes
sticky non-evaluable after any unrecovered local ingress, WAL, evidence or IPC
gap. Canonical account updates continue to be forwarded so PR1B does not change
AccountStateCore authority; incomplete transaction candidates are suppressed.
A local stall never triggers a Yellowstone reconnect or claims provider
backfill.

The slow-sink measurements were:

```text
slow WAL enqueue: 611,032 ns
physical writer elapsed: 8,166,808 ns
physical writer calls/waits: 2/2, isolated from event worker
slow IPC enqueue: 8,016 ns
IPC worker waits: 0
```

### Verification on the final diff

Passed:

```text
cargo fmt --all --check
git diff --check
cargo test -p ghost-core ingest_integrity -- --nocapture
cargo test -p seer --lib pr1b_ -- --nocapture
cargo test -p seer --lib one_continuous_saturation_episode_produces_one_deterministic_gap -- --nocapture
cargo test -p seer --lib bounded_ingress_saturation_is_nonblocking_and_emits_one_gap -- --nocapture
cargo test -p seer --lib ipc::tests -- --nocapture
cargo test -p seer --lib provider_metadata -- --nocapture
cargo test -p seer --lib account_update_preserves_provider_and_optional_transaction_signature -- --nocapture
cargo test -p ghost-launcher --lib local_coverage_gap_replays_as_audit_only_record -- --nocapture
cargo check -p ghost-brain --tests
cargo test --release -p seer --lib pr1b_hot_path_harness -- --ignored --nocapture --test-threads=1
timeout 900s cargo build --release --workspace
```

The additive `LocalCoverageGap` WAL replay test counts it as audit evidence and
performs no state mutation.

Known baseline failures remain visible:

- `timeout 300s cargo test -p seer --lib -- --test-threads=1` timed out after
  reporting the same 14 PumpPortal/Seer failures listed in the B0 receipt;
- `cargo check -p ghost-launcher --tests` fails on existing `E0063` fixtures
  that construct `PoolTransaction` without five fields added before PR1B;
- `timeout 600s cargo test --workspace -- --test-threads=1` fails with the same
  `E0063` fixture class.

No unrelated fixture repair is included in PR1B.

### Scope boundary

PR1B changes transport, parsing work ownership, sink scheduling and typed local
coverage evidence only. It does not change strategy, scores, MFS, Gatekeeper,
entry/exit thresholds, quote math, execution, NLN role, runtime authority or
AccountStateCore arbitration.

Intentionally deferred:

- PR1C: typed AccountObservationArbiter and provider/account mutation
  arbitration;
- PR1D: Observation Ledger and raw/NLN reconciliation;
- proof-based recovery of a local processing gap;
- provider reconnect/backfill state machine for semantically provable provider
  gaps.
