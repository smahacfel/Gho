# RAPORT P3.7-J3K4 R15 Bounded R1 Smoke

Date: 2026-05-21

Status:

```text
R15-bounded-j3k4-r1 = MINIMAL PASS / DIAGNOSED
J3K4 structured simulation diagnostics = RUNTIME VALIDATED
probe transport / entry path = PASS
exact decision/V3 join = PASS
simulation errors = DIAGNOSED_NOT_CLEAN
lifecycle/on-chain labels = NOT_VALIDATED
collection / Phase B / P2 / live / tuning = HOLD / NO-GO
```

## Run

Config:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k4-r1.toml
```

The run was stopped by the monitoring wrapper after reaching the configured
diagnostic transport target:

```text
stop_reason = transport_limit_reached
```

This was intentional. The run was a bounded diagnostic gate, not a collection
run.

## Counts

```text
probe_selection_rows = 12
probe_skips_rows = 2
probe_transport_rows = 10
probe_shadow_entry_rows = 10
probe_lifecycle_rows = 0
active_buy_rows = 0
```

Replay:

```text
strict_full_replay = full_replay_ok
bad_rows = 0
```

Join-key audit:

```text
probe_transport exact_decision_v3_join = 10/10
probe_entry exact_decision_v3_join = 10/10
probe_required_exact_decision_v3_join_coverage = 1.0
feature_hash_mismatch = 0
policy_hash_mismatch = 0
```

Entry materialization:

```text
entry_materialized = 8
simulation_error = 2
transport_only_missing_token_quantity = 0
```

## Simulation Errors

Two simulation errors were observed:

```text
simulation_slippage_or_price_mismatch:custom_6002 = 1
simulation_account_layout_mismatch:custom_2006 = 1
```

### Custom 6002

The Pump.fun `TooMuchSolRequired` row remains classified as:

```text
simulation_slippage_or_price_mismatch
```

The row used:

```text
buy_variant = legacy_buy
token_param_role = token_amount
probe_amount_lamports = 7000000
entry_token_amount_raw = 124803725982
min_tokens_out = 99842980785
```

This is an amount/price/slippage diagnostic class. It is not an account-readiness
failure.

### Custom 2006

J3K4 successfully populated the Anchor constraint diagnostics:

```text
program = pumpfun
custom_code = 2006
category = simulation_account_layout_mismatch
account_role = creator_vault
actual = 4D8hkwjsgvn5hrQgJULqxuh5hSX3UEUEe2U9nWpTiyTP
expected = GdZspP3tLaQQ5jrFixZ2xPmWjshMWEX6K9ynkx2BiXLM
```

This means the simulated transaction carried a `creator_vault` PDA derived from
the local creator identity, while the Pump.fun program expected a different
creator-vault PDA.

## Interpretation

The probe plane is now validated through:

```text
V3/MFS decision row
-> probe selection
-> probe transport
-> probe shadow entry
-> exact ab_record_id/probe_id/hash continuity
```

The remaining blockers are not join-key or basic transport/entry plumbing. They
are execution-parameter and route/account identity quality:

- `custom_6002`: amount/slippage/price mismatch class;
- `custom_2006`: creator-vault route/account identity mismatch class.

## Decision

Do not scale to collection from this smoke alone.

Next repair path:

```text
P3.7-J3K5 Creator-Vault Source Authority / Amount Guard Decision
```

The next step should decide whether to:

- narrow eligibility for rows whose creator identity is not route-authoritative;
- add decision-time-safe creator-vault/source materialization;
- keep `custom_2006` as a diagnosed simulation-error class under strict
  stop-loss gates if it remains rare;
- add an amount/slippage guard if `custom_6002` grows in the next bounded sample.

Collection, Phase B, P2, live, active policy changes, IWIM changes and threshold
tuning remain out of scope.
