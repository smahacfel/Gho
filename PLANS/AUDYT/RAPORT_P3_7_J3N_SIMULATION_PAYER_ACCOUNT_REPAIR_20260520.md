# P3.7-J3N Simulation Payer Account Repair

Status: CODE-LEVEL REPAIR READY FOR R15-R8H SMOKE

## Context

R15-r8f was stopped early after a useful blocker appeared. The run produced
one ordinary shadow BUY simulation row, but the simulation ended with:

```text
err = AccountNotFound
error_class = data_problem
payer_provenance = ephemeral
```

The failed prepared-buy log identified the fee payer as an ephemeral key:

```text
payer = 5wpV22LLfdCHvWJ9yS7sbEsEGMxShkoGd9F5LyrGsSMC
payer_provenance = ephemeral
```

RPC checks showed that this ephemeral payer account did not exist on-chain.
That makes it a plausible source for the `AccountNotFound` simulation failure.

## Configured Payer Check

The local rollout environment points at:

```text
GHOST_TRIGGER_KEYPAIR_PATH = wallets/shadow-burnin-test.json
```

The keypair resolves to:

```text
configured_pubkey = 9MCkR8iiQLRxS242CbQijfaKT5AGNr2bWoSsXbQqvbaw
account_exists = true
lamports = 47172000
```

The old buy-heavy profile uses:

```text
[trigger.shadow_run]
payer_strategy = "configured"
```

This matches the remembered historical shadow-burnin contract: shadow
simulation uses a configured, chain-visible fee payer, while live sender remains
disabled.

## Repair

Added a fresh R15-r8h smoke profile:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8h.toml
```

Key contract:

```text
[trigger]
entry_mode = "shadow_only"
keypair_path = "../../wallets/shadow-burnin-test.json"

[trigger.shadow_run]
payer_strategy = "configured"

[execution]
execution_mode = "shadow"

[seer]
funding_lane_mode = "disabled"
```

The profile keeps all J3 probe bounds unchanged:

```text
max_probes_per_run = 5
max_probes_per_minute = 5
max_concurrent = 1
max_scan_concurrent = 8
max_probe_candidates_scanned_per_run = 1000
```

Additionally, `ShadowBuySimulationRecord` now carries an optional additive
`payer_pubkey` field. Legacy rows without this field still deserialize. Future
shadow simulation failures will no longer require correlating `buys.jsonl` with
`system.log` just to identify the fee payer.

## Non-Goals Preserved

- No collection.
- No P2.
- No live sender.
- No active policy change.
- No IWIM change.
- No threshold tuning.
- No bypass of strict execution-account precheck.
- No treatment of `AccountNotFound` as success.

## Next Gate

Run a short bounded R15-r8h smoke and stop early on the first concrete blocker:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8h.toml
```

Expected outcome:

- V3/MFS strict replay remains OK.
- Exact decision/V3 hash continuity remains OK.
- Active shadow `AccountNotFound` is no longer caused by a missing ephemeral
  payer.
- If another `AccountNotFound` appears, the row must include `payer_pubkey` and
  `payer_provenance`.

Full/bounded collection and Phase B remain HOLD until R15-r8h reaches
transport/entry PASS or produces a new precise NOT_READY diagnosis.
