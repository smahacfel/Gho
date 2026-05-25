# RAPORT P3.7-X2S - Working Builder Parity Shadow Simulation Smoke - 2026-05-25

## Status

Decision: BLOCK

Runtime validation was not started. The smoke was blocked during preflight because the required secret environment for a strict X2S run was not available as process env.

This is not a P3.7-X2 code-level failure. It is a safe-start blocker for X2S.

## Commit Baseline

- X2 commit: `76b9264 Implement P3.7 X2 working builder parity mode`
- Push target: `origin/main`
- Push result: `9e0b28a..76b9264 main -> main`

## Smoke Contract

X2S must verify whether P3.7 shadow/probe can build and simulate the working `PreparedBuyRequest` family used by the historical Helius Sender BUY path, without legacy fallback mutation and without `selected_route_handoff_mismatch`.

Hard boundaries:

- no live execution
- no Helius Sender submission
- no P2/live activation
- no R18 before X2S
- no L2D2
- no Gatekeeper change
- no threshold tuning
- no `legacy_buy` fallback revival
- no ABI discovery as primary path

## Config Used For Preflight

Local smoke config:

`configs/rollout/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke.local.toml`

This config is intentionally local/untracked and contains no secrets.

Relevant fields:

- `[execution].execution_mode = "shadow"`
- `[trigger].entry_mode = "shadow_only"`
- `[trigger.shadow_run].payer_strategy = "ephemeral"`
- `[trigger.shadow_run].max_concurrent = 1`
- `[p37_shadow_probe].enabled = true`
- `[p37_shadow_probe].p37_execution_builder_mode = "working_builder_parity"`
- `[p37_shadow_probe].max_probes_per_run = 15`
- `[p37_shadow_probe].max_concurrent = 1`
- no `[seer].helius_endpoint`
- no live Sender endpoint

Secret policy:

- required process env: `CHAINSTACK_GRPC_ENDPOINT`, `CHAINSTACK_GRPC_TOKEN`, `CHAINSTACK_RPC_URL`
- these variables were missing in the process environment
- local `.env` was intentionally bypassed by running with `GHOST_ENV_FILE=/tmp/ghost-x2s-empty.env`
- this prevents accidental use of local secret fallbacks and keeps X2S fail-closed

## Preflight Command

```bash
GHOST_ENV_FILE=/tmp/ghost-x2s-empty.env RUSTFLAGS=-Awarnings cargo run -p ghost-launcher --bin ghost-launcher -- --config configs/rollout/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke.local.toml --preflight
```

Result:

```text
Error: shadow-capable production profiles require a real [trigger.shadow_run].shadow_rpc_url (or GHOST_TRIGGER_SHADOW_RPC_URL); placeholder values are not allowed for launcher shadow transport
```

Interpretation:

- the launcher failed closed before runtime
- no Yellowstone stream was started
- no shadow/probe rows were emitted
- no transaction was simulated
- no Helius Sender was initialized
- no live Sender submission occurred

## Required X2S Fields

All row counters below are pre-runtime zeroes. They are not runtime evidence; they only record that no X2S dispatch occurred after the safe-start blocker.

| Field | Value | Source |
|---|---:|---|
| `working_builder_parity_rows` | 0 | no runtime rows emitted |
| `working_builder_request_built_rows` | 0 | no runtime rows emitted |
| `working_builder_buy_variant_counts` | `{}` | no runtime rows emitted |
| `working_builder_rpc_manifest_hash_coverage` | `0/0` | no runtime rows emitted |
| `working_builder_sender_manifest_hash_coverage` | `0/0` | no runtime rows emitted |
| `working_builder_manifest_contains_bcv2_rows` | 0 | no runtime rows emitted |
| `working_builder_manifest_ready_rows` | 0 | no runtime rows emitted |
| `working_builder_manifest_missing_required_rows` | 0 | no runtime rows emitted |
| `legacy_fallback_attempted_rows` | 0 | no runtime rows emitted |
| `selected_route_handoff_mismatch_rows` | 0 | no runtime rows emitted |
| `successful_entry_rows` | 0 | no runtime rows emitted |
| `lifecycle_eligible_rows` | 0 | no runtime rows emitted |
| `post_simulation_account_not_found_rows` | 0 | no runtime rows emitted |

## PASS/BLOCK/FAIL Classification

BLOCK.

Reason:

`CHAINSTACK_GRPC_ENDPOINT`, `CHAINSTACK_GRPC_TOKEN`, and `CHAINSTACK_RPC_URL` were not available as process environment variables for the strict X2S run, and local `.env` fallback was intentionally disabled.

This is not PASS-A because no working builder request rows were produced.

This is not PASS-B because no runtime rows exist and no final working builder required-account blocker was observed.

This is not FAIL because none of the X2 failure predicates were observed:

- no legacy fallback attempt occurred
- no buy variant mutation was observed
- no `selected_route_handoff_mismatch` was observed
- no post-simulation `AccountNotFound` occurred
- no rpc/sender manifest parity inconsistency was observed

No such predicates were observed because runtime did not start.

## Required Next Step

Rerun X2S after exporting the required environment variables as process env:

```bash
export CHAINSTACK_GRPC_ENDPOINT=...
export CHAINSTACK_GRPC_TOKEN=...
export CHAINSTACK_RPC_URL=...
```

Then run the same local smoke config without using a local secret config file and without enabling live Sender.

No R18 should be started before X2S produces PASS-A or PASS-B.
