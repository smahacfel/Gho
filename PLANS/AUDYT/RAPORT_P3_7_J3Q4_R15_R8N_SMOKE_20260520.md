# RAPORT P3.7-J3Q4 R15-r8n Counterfactual Probe Smoke

Date: 2026-05-20

## Verdict

```text
R15-r8n smoke: MINIMAL PASS / DIAGNOSED
J3Q4 simulation-error classification: PASS
counterfactual probe transport/entry path: RUNTIME VALIDATED
lifecycle/on-chain label path: NOT VALIDATED
small bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

R15-r8n is the first counterfactual probe smoke that produced real probe
transport and probe shadow-entry rows with exact V3 decision/hash continuity,
without producing an active BUY artifact.

## Inputs

- config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8n.toml`
- namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8n`
- decision root: `logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8n/decisions`
- shadow root: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8n`

## Runtime Summary

```text
probe_selection_rows = 17
probe_skip_rows = 20
probe_transport_rows = 5
probe_shadow_entry_rows = 5
probe_lifecycle_rows = 0
active_buys_jsonl = missing
```

Probe transport outcomes:

```text
counterfactual_shadow_probe_simulated = 4
counterfactual_shadow_probe_simulation_error = 1
```

Probe shadow-entry outcomes:

```text
counterfactual_shadow_probe_simulated = 4
counterfactual_shadow_probe_simulation_error = 1
```

All probe transport rows carry:

```text
dispatch_source = counterfactual_shadow_probe
ab_record_id present = 5/5
probe_id present = 5/5
v3_feature_snapshot_hash present = 5/5
v3_policy_config_hash present = 5/5
```

## Replay And Join-Key Status

Strict replay was checked against both decision-log planes that carried V3
payloads in the R15-r8n namespace:

```text
legacy_live v3 rows = 25
legacy_live strict replay bad_rows = 0
v25_shadow v3 rows = 2
v25_shadow strict replay bad_rows = 0
```

Join-key audit status for the probe plane:

```text
probe_readiness = ready_for_probe_transport_entry_join
probe_join_key_acceptance = pass
probe_join_quality = exact_probe_id_and_ab_record_id
probe_decision_join_acceptance = pass
probe_required_exact_decision_v3_join_coverage = 1.0
probe_selection exact_decision_v3_join = 17/17
probe_transport exact_decision_v3_join = 5/5
probe_entry exact_decision_v3_join = 5/5
```

The non-probe shadow readiness remains `not_ready` because this smoke did not
produce normal shadow BUY transport/entry/lifecycle artifacts. That is expected
for this counterfactual probe smoke and is not used as the J3Q4 gate.

## Simulation Error Classification

One probe produced a simulation instruction error:

```text
err = InstructionError(3, Custom(6002))
program_id = 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P
program_name = pumpfun
program_error_name = too_much_sol_required
category = simulation_slippage_or_price_mismatch
route_kind = legacy_buy
```

The simulation log tail identifies the Pump.fun Anchor error:

```text
Error Code: TooMuchSolRequired
Error Number: 6002
Error Message: slippage: Too much SOL required to buy the given amount of tokens.
Left: 7000000
Right: 11425995
```

This is not an `AccountNotFound` class, not a hash-continuity class, and not
an active BUY mutation. It is an amount/slippage mismatch for one probe request.

## Decision

R15-r8n validates the counterfactual probe transport/entry path at runtime:

- V3 replay payloads are replayable.
- Probe selection, transport and entry exact-join back to V3 decision rows.
- Probe rows are marked `dispatch_source=counterfactual_shadow_probe`.
- Active BUY artifacts remain absent.
- The remaining simulation error is classified as
  `simulation_slippage_or_price_mismatch`.

Do not start a larger collection yet. The next narrow step is:

```text
P3.7-J3Q5 Probe Amount / Slippage Semantics
```

J3Q5 should decide whether `TooMuchSolRequired` rows are an expected
classified probe error, a probe quote/amount construction issue, or a
smoke-profile amount/slippage configuration issue. Collection remains on hold
until that decision is documented.
