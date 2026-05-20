# RAPORT P3.7-J3Q5 Probe Amount / Slippage Diagnostic

Date: 2026-05-20

## Verdict

```text
P3.7-J3Q5 diagnostic: PASS / INCOMPLETE TOKEN-PARAM CONTEXT
R15-r8n probe transport/entry: MINIMAL PASS
TooMuchSolRequired classification: DIAGNOSED AS AMOUNT/SLIPPAGE MISMATCH
small bounded collection: CONDITIONAL HOLD
next gate: one tiny diagnostic smoke with token-param transport fields
Phase B / P2 / live / tuning: NO-GO
```

Q5 confirms that the remaining R15-r8n simulation error is not a join-key,
account-readiness, payer, live, or active-BUY mutation problem. The error is a
Pump.fun `TooMuchSolRequired` amount/slippage mismatch.

Do not scale directly to a 25+ probe collection from R15-r8n alone because the
R15-r8n transport rows were emitted before probe transport carried
`buy_variant`, `token_param_role`, `entry_token_amount_raw`, and
`min_tokens_out`. Those fields were added after R15-r8n and are required to
separate stale quote / amount-too-large / token-param mismatch cleanly.

## Input

- namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8n`
- transport: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8n/probe_transport.jsonl`
- entries: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8n/probe_shadow_entries.jsonl`
- selection: `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8n/probe_selection.jsonl`
- Q4 simulation report: `PLANS/AUDYT/RAPORT_P3_7_J3Q4_R15_R8N_SIMULATION_ERROR_ANALYSIS_20260520.md`

## Probe Row Comparison

All five R15-r8n probe transport rows share the same high-level decision and
probe setup:

```text
probe_bucket = v3_reject_manipulation_contradiction
active_verdict_type = TIMEOUT_PHASE1_INSUFFICIENT
v3_shadow_verdict = REJECT
v3_shadow_reason_code = REJECT_V3_MANIPULATION_CONTRADICTION
route_kind = legacy_buy
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
curve_data_known = true
```

| probe | outcome | entry_price | error | custom | required/max |
| --- | --- | ---: | --- | ---: | ---: |
| `1434d77f46` | `counterfactual_shadow_probe_simulated` | `3.1520848229365265e-08` | none | none | none |
| `2952383241` | `counterfactual_shadow_probe_simulation_error` | `5.853459361139875e-08` | `TooMuchSolRequired` | `6002` | `1.632285` |
| `16cb856e0b` | `counterfactual_shadow_probe_simulated` | `2.8618103808659064e-08` | none | none | none |
| `75a656ffb1` | `counterfactual_shadow_probe_simulated` | `2.902501655646311e-08` | none | none | none |
| `7a7958f36a` | `counterfactual_shadow_probe_simulated` | `1.1175409377339196e-08` | none | none | none |

The error row is the highest entry-price row in this five-row sample. Its
simulation logs show:

```text
Error Code: TooMuchSolRequired
Error Number: 6002
Left: 7000000
Right: 11425995
```

Interpretation:

```text
requested/max SOL = 7_000_000 lamports
program-required SOL = 11_425_995 lamports
required_over_max = 1.632285
```

The program reached the Pump.fun buy instruction and rejected the request
because the encoded token/amount constraint required more SOL than the probe
allowed.

## Diagnostic Limits

R15-r8n was generated before the post-Q4 transport schema carried these fields:

```text
buy_variant
token_param_role
entry_token_amount_raw
min_tokens_out
```

Therefore Q5 can classify the error family but cannot fully choose between:

- `simulation_amount_too_large`
- `simulation_quote_stale`
- `simulation_token_param_mismatch`
- `simulation_buy_variant_mismatch`

The current best classification is:

```text
simulation_slippage_or_price_mismatch
```

with likely sub-class:

```text
amount_or_token_param_exceeded_max_sol
```

## Decision

Do not treat `TooMuchSolRequired` as a strategic failure. R15-r8n proves the
counterfactual probe plane can produce transport and entry rows with exact V3
decision continuity.

Do not start the first 25+ probe collection yet. The next safe gate is one
tiny diagnostic smoke using the already-added transport token-param fields:

```text
P3.7-J3Q5b token-param-aware smoke
```

Minimal acceptance for Q5b:

```text
probe_transport_rows > 0
probe_entry_rows > 0
exact decision/V3 join = 100%
transport rows include buy_variant/token_param_role/entry_token_amount_raw/min_tokens_out
TooMuchSolRequired rows, if any, can be sub-classified
active BUY remains absent
no live/P2 path touched
```

If Q5b again shows only an isolated low-rate `TooMuchSolRequired` class and the
sub-class is clear, a small bounded collection can start with strict error
class reporting.

If Q5b shows systematic `TooMuchSolRequired`, repair the probe amount/quote
construction or run a tiny amount-variant smoke before collection.

## Non-Goals

- No active policy change.
- No IWIM change.
- No threshold tuning.
- No P2/live.
- No Phase B selector claim.
- No treating probe rows as BUY decisions.
