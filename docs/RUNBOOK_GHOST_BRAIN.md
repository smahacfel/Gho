# Ghost-Brain Integration Runbook

## Overview

After a successful BUY, `ghost-launcher` delegates the entire post-buy lifecycle to
`ghost-brain` via the **PostBuyRuntime adapter** (`post_buy_runtime.rs`).

### Architecture (SSOT)

```text
ghost-launcher (orchestrator + adapter)
  ├─ oracle_runtime.rs  → emits PostBuySubmitted on Ok(signature)
  └─ post_buy_runtime.rs  → stateless adapter, maps PostBuySubmitted → CandidateRef

ghost-brain (SSOT for post-buy lifecycle)
  └─ PaperPositionLifecycle::run()  ← single entrypoint, self-drives:
       ├─ PaperBroker    — entry/exit fill simulation (200–400 ms delay)
       ├─ AemRuntime     — management decisions (TP / SL / Hold / Panic)
       └─ EventEmitter   — JSONL event output
```

**Key invariant:** ghost-launcher contains ZERO lifecycle logic — no tick loop, no fill
polling, no exit decisions. All of this lives in `ghost-brain::PaperPositionLifecycle`.

---

## JSONL Output Location

ghost-brain's `EventWriter` writes rotated JSONL files into the configured output
directory:

```
<events_output_dir>/exec_<run_id>_<date>_<seq>.jsonl
```

Default directory (from `config.toml`):

```
datasets/events/
```

The `run_id` is generated at startup: `launcher-<timestamp_ms>`.

Example file path:

```
datasets/events/exec_launcher-1708900000000_2026-02-26_0000.jsonl
```

---

## Event Format

Each line is a JSON object with two top-level keys:

```json
{
  "envelope": {
    "run_id": "launcher-1708900000000",
    "lane": "Paper",
    "candidate_id": "TestMint_TestPool_0",
    "position_id": "paper-pos-1",
    "position_epoch": 1,
    "event_id": "...",
    "event_time_ms": 1708900001234,
    "slot": null,
    "quote_id": null,
    "command_id": null,
    "order_id": null
  },
  "kind": {
    "type": "AemTick",
    "payload": { ... }
  }
}
```

`kind.type` is the event discriminator. `kind.payload` contains event-specific data.

---

## Lifecycle Events (expected order for one position)

| # | Event Type          | Emitted By              | Proves                              |
|---|---------------------|-------------------------|--------------------------------------|
| 1 | `Candidate`         | PaperPositionLifecycle  | Gatekeeper PASS, pool accepted       |
| 2 | `EntrySubmitted`    | PaperPositionLifecycle  | Entry order sent to PaperBroker      |
| 3 | `EntryFilled`       | PaperPositionLifecycle  | Paper fill confirmed (200–400 ms)    |
| 4 | `PositionOpened`    | PaperPositionLifecycle  | Position registered in AemRuntime    |
| 5 | `AemTick` (×N)      | AemRuntime (ghost-brain)| Tick loop running, features computed |
| 6 | `ManagementDecision`| AemRuntime (ghost-brain)| AEM produced a regime-based decision |
| 7 | `ExitSubmitted`     | PaperPositionLifecycle  | Exit order sent to PaperBroker       |
| 8 | `ExitFilled`        | PaperPositionLifecycle  | Exit fill confirmed                  |
| 9 | `PositionClosed`    | PaperPositionLifecycle  | Position fully closed                |
|10 | `ManagementOutcome` | PaperPositionLifecycle  | Final outcome recorded               |

**If `AemTick` is missing, the integration is broken.**

---

## How to Verify (Paper Mode)

### 1. Run ghost-launcher in paper mode

Ensure `config.toml` has:

```toml
[execution]
execution_mode = "Paper"

[execution.events]
output_dir = "datasets/events"
```

### 2. Wait for a BUY to trigger

After Gatekeeper PASS → BUY bundle success, look for the log line:

```
PostBuyRuntime: received PostBuySubmitted, delegating to ghost-brain
```

Followed by ghost-brain lifecycle logs:

```
PaperLifecycle: entry submitted
PaperLifecycle: position opened
PaperLifecycle: AEM tick with decision
PaperLifecycle: position lifecycle complete
```

### 3. Check JSONL output

```bash
# Find the latest JSONL file
ls -lt datasets/events/exec_*.jsonl | head -1

# Count event types
cat datasets/events/exec_*.jsonl | \
  jq -r '.kind.type' | sort | uniq -c | sort -rn
```

Expected output (for one position):

```
  5 AemTick
  1 Candidate
  1 EntrySubmitted
  1 EntryFilled
  1 PositionOpened
  1 ManagementDecision
  1 ExitSubmitted
  1 ExitFilled
  1 PositionClosed
  1 ManagementOutcome
```

### 4. Verify lane is Paper

```bash
cat datasets/events/exec_*.jsonl | jq -r '.envelope.lane' | sort | uniq
```

Should output only: `Paper`

### 5. Verify no gaps

```bash
# Extract position_id and event types in order
cat datasets/events/exec_*.jsonl | \
  jq -r '[.envelope.position_id // "none", .kind.type] | @tsv'
```

All events for the same `position_id` should appear in the order listed above.

---

## PostBuySubmitted Contract

The adapter relies on `oracle_runtime.rs` emitting `GhostEvent::PostBuySubmitted`
immediately after a successful buy bundle (PR #4, `oracle_runtime.rs:2468–2487`).

This event carries:

| Field            | Source                        | Required |
|------------------|-------------------------------|----------|
| `pool_amm_id`    | Pool AMM ID (string)          | ✅       |
| `base_mint`      | Base token mint (string)      | ✅       |
| `signature`      | Transaction signature         | ✅       |
| `amount_sol`     | Trade value in SOL            | ✅       |
| `tip_lamports`   | Jito tip in lamports          | ✅       |
| `lane`           | Execution lane (paper/live)   | ✅       |
| `epoch_id`       | Monotonic epoch counter       | ✅       |

The emission point is in `oracle_runtime.rs` at the `Ok(signature)` branch after
`execute_buy_bundle`, using `GhostEvent::post_buy_submitted(...)`.

If any field is missing or zero, the adapter will still function but may produce
degraded event data (e.g. fallback Pubkeys for non-parseable pool/mint strings).

### Contract validation

The integration test (`post_buy_runtime_integration.rs`) validates:
- `PostBuySubmitted` triggers ghost-brain lifecycle
- All 7 fields are consumed and mapped to `CandidateRef`
- Event ordering is deterministic: Candidate → EntrySubmitted → EntryFilled →
  PositionOpened → AemTick(≥3) → ManagementDecision → ExitSubmitted → ExitFilled →
  PositionClosed

---

## Troubleshooting

| Symptom                              | Cause                                      | Fix                                        |
|--------------------------------------|--------------------------------------------|--------------------------------------------|
| No JSONL files                       | EventWriter failed to initialize           | Check output_dir exists and is writable    |
| Events missing `AemTick`             | AEM not ticking or disabled                | Verify `aem_config.enabled = true`         |
| No `ManagementDecision`              | Too few ticks before exit                  | Increase `max_ticks_before_exit`           |
| `PostBuySubmitted` not received      | Event bus subscriber started too late       | Ensure bridge subscribes BEFORE oracle     |
| `send failed: no receivers`          | No subscribers on broadcast channel        | Check startup ordering in `main.rs`        |
