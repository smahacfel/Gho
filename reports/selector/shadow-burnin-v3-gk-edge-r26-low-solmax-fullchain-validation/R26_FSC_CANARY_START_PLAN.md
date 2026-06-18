# R26_FSC_CANARY_START_PLAN

Status: READY_TO_BUILD_AND_START_AFTER_USER_APPROVAL
Generated: 2026-06-12

## Purpose

R26 purpose:

```text
R26_CANARY_FUNDING_FULL_CHAIN
purpose = FSC canary + data collection only
not BUY validation
```

R26 is not a validation run for any old BUY selector candidate.

Do not promote:

- `LOW_EARLY_PRICE_LAST_BROAD_SELECTOR_V1`
- `LOW_BUY_RATIO_BROAD_SELECTOR_V1`
- `LOW_EARLY_SOL_MAX_BROAD_SELECTOR_V1`
- any other old Segment Lab broad candidate

Non-goals:

- no Gatekeeper policy change
- no BUY/REJECT/TIMEOUT mutation
- no execution change
- no send path change
- no threshold tuning
- no production promotion

## Config

Config path:

```text
configs/rollout/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation.toml
```

Scope/path family:

```text
shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation
```

The config header has been corrected to describe R26 as FSC canary/data collection only, not old BUY-edge validation.

## Config Delta

The required functional seer shape is:

```toml
[seer]
source_mode = "grpc"
stream_mode = "single_global"
funding_lane_mode = "full_chain"

[seer.program_streams]
enabled = true
max_streams = 2
enabled_topics = [
  "solana.pump_fun.buy",
  "solana.pump_fun.buy_exact_sol_in",
]
```

Compared with R25, the intended functional delta is:

```text
[seer]
- funding_lane_mode = "disabled"
+ funding_lane_mode = "full_chain"
```

Other relevant R26 config values:

```text
[metrics]
port = 9128

[gui_backend]
port = 8828

[oracle]
decision_log_path = "../../logs/rollout/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/decisions"

[execution.events]
output_dir = "../../datasets/events/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation"

[seer.program_streams]
artifact_capture_dir = "logs/nln_capture/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation"
```

## Expected Ingest Shape

Expected 4-source shape:

1. raw gRPC primary: `grpc_global_stream`
2. raw gRPC funding lane: `grpc_funding_lane_full_chain`
3. NLN Program Stream: `solana.pump_fun.buy`
4. NLN Program Stream: `solana.pump_fun.buy_exact_sol_in`

Program streams are route/account evidence. They do not replace full-chain funding evidence.

## Pre-Start Requirements

Tier A cleanup removed `/root/Gho/target`. Therefore a fresh release build is required before start:

```bash
cargo build --release -p ghost-launcher
```

Start only through the lifecycle launcher, not by directly invoking the binary.

Recommended launch command after build:

```bash
python3 scripts/start_selector_lifecycle_run.py \
  --scope shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation \
  --config configs/rollout/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation.toml \
  --tmux-session selector_r26_fsc_fullchain_canary \
  --min-free-gb 25 \
  --event-canary-seconds 900 \
  --lifecycle-proof-timeout-seconds 3600 \
  --min-reporter-rows 1
```

Do not start R26 unless disk free remains above the chosen safety threshold.

## Acceptance Gates

R26 canary PASS requires all of:

- `grpc_global_stream` connected.
- `grpc_funding_lane_full_chain` connected.
- `solana.pump_fun.buy` connected.
- `solana.pump_fun.buy_exact_sol_in` connected.
- no `ResourceExhausted`.
- no provider stream limit rejection.
- no reconnect storm.
- decision rows are emitted.
- primary decisions still have ingest from `grpc_global_stream`.
- `fsc_authoritative_funding_stream_available = 1` after warmup.
- FSC coverage/warmup gate does not flap after stabilization.
- funding lane does not degrade primary ingest.
- Gatekeeper BUY/REJECT/TIMEOUT semantics remain unchanged.
- execution/send path remains unchanged.

## Kill Conditions

Stop R26 canary if any of:

- `ResourceExhausted`.
- provider stream limit exceeded.
- primary `grpc_global_stream` degraded.
- decision rows stop being emitted.
- reconnect storm.
- disk free below safety threshold.
- FSC warmup flaps without stabilization.
- funding lane negatively affects route evidence or primary ingest.
- launcher lifecycle proof fails.

## Log Paths

Primary rollout logs:

```text
logs/rollout/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/system.log
logs/rollout/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/oracle.log
logs/rollout/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/decisions/
```

Program/funding capture artifacts:

```text
logs/nln_capture/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/
```

Event dataset output:

```text
datasets/events/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/
```

Shadow execution evidence:

```text
logs/shadow_run/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/shadow_entries.jsonl
logs/shadow_run/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/shadow_lifecycle.jsonl
```

Launcher report root:

```text
reports/selector/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/
```

## Metrics / Strings To Observe

Inspect logs/metrics for:

```text
grpc_global_stream
grpc_funding_lane_full_chain
solana.pump_fun.buy
solana.pump_fun.buy_exact_sol_in
ResourceExhausted
stream limit
reconnect
fsc_authoritative_funding_stream_available
FundingLaneUnavailable
funding_source_concentration
gatekeeper_v2_decisions
selector_shadow_score_v1
```

Decision rows expected under:

```text
logs/rollout/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation/decisions/**/gatekeeper_v2_decisions.jsonl
```

## First Health Check

Within the first 100 seconds after launch:

1. Confirm tmux session exists.
2. Confirm process is alive.
3. Confirm system/oracle logs are growing.
4. Confirm no immediate `ResourceExhausted` or stream-limit errors.
5. Confirm decision directory exists or is beginning to populate.
6. If the canary is still legitimately warming up, leave it in tmux and stop active monitoring until explicitly asked again.

## Canary Interpretation

PASS:

```text
R26_FSC_CANARY_FUNDING_FULL_CHAIN_PASS
```

Meaning: full-chain funding lane appears operational and R26 can continue as data collection.

FAIL:

```text
R26_FSC_CANARY_FUNDING_FULL_CHAIN_FAIL
```

Meaning: stop run, preserve logs, diagnose funding lane/provider/ingest issue. Do not interpret as model failure.

## Business Boundary

R26 does not validate a BUY edge. Business-label selector validation remains blocked because current R21/R23/R24/R25 business-label diagnostics ended with:

```text
NO_BUSINESS_BUY_SELECTOR_FOUND
NO_STRONG_STOP_VETO_FOUND
TIMEOUT_RISK_VETO_CANDIDATE_FOUND
FEATURE_FAMILY_INSUFFICIENT_FOR_25_25_60_BUY_SELECTOR
```

R26 can only produce new data for later offline analysis with active full-chain FSC/funding evidence.
