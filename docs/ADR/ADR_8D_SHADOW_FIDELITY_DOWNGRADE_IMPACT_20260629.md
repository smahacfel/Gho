# ADR-8D: Shadow Fidelity Downgrade Impact 2026-06-29

## Status

Accepted as downgrade decision.

## Decision

Final measurement verdict:

```text
SHADOW_REPLAY_LIFECYCLE_MISMATCH
```

Previous strategy reports must be downgraded where they rely on shadow data as live-equivalent evidence or as unified lifecycle/replay position truth.

## Context

The P0 Shadow Burnin Fidelity Audit found that `shadow_exit_replay_v1` can be reconstructed as an offline path-label artifact, but `shadow_exit_replay_v1` and `shadow_lifecycle` cannot be treated as one unified position truth. The same audit also confirmed that shadow evidence does not model or prove live landing, failed transactions, entry/exit slippage, own trade impact, AMM fees, blockhash/Jito/priority fee behavior, or quote/fill divergence.

R51 remains `ACTIVE_PARTIAL / DIAGNOSTIC_ONLY`. It is not strategy evidence and must not be interpreted as RCE proof while shadow fidelity issues remain unbounded.

## Consequences

- shadow is not live-equivalent;
- shadow entry price is not proven as live fill;
- shadow exit result is offline path/label evidence, not executable sell fill;
- replay/lifecycle cannot be treated as one unified position truth;
- 2s/3s conclusions are sparse approximation only;
- 300s/500s conclusions are not evaluable;
- live runtime approval remains false;
- shadow_close_only approval remains false;
- active close approval remains false;
- R51 is ACTIVE_PARTIAL / DIAGNOSTIC_ONLY, not strategy evidence.

## Downgrade impact by report family

### ORG-A0

ORG-A0 remains no-runtime, but only under offline path-label measurement assumptions.

### R48/R2 exit matrix

R48/R2 exit matrix remains no-runtime, but not live-equivalent.

### TSV2 A1/A2/A3

TSV2 A1/A2/A3 remains diagnostic only; lifecycle/replay mismatch blocks active close proof.

### EIX

EIX remains data-blocked.

### RTP-A0

RTP-A0 remains diagnostic only; not live-equivalent.

### RUG-MARKUP-A0

RUG-MARKUP-A0 remains no-runtime under component replay, not a live fill proof.

### RCE-A0

RCE-A0 remains blocked by missing surface; R51 cannot be interpreted until shadow fidelity issues are explicitly bounded.

## Explicit citation restriction

Previous reports must not be cited as proof of live PnL, executable fills, live slippage behavior, or real landing outcome.

Any downstream report that cites ORG-A0, R48/R2, TSV2 A1/A2/A3, EIX, RTP-A0, RUG-MARKUP-A0, or RCE-A0 must carry the downgrade constraints forward.

## Runtime boundary

This ADR is documentation-only. It does not change runtime behavior, BUY/REJECT, Gatekeeper policy, selector runtime, TX/Jito/live path, `shadow_close_only`, or active close.

## Approval boundary

```yaml
live_runtime_approval: false
shadow_close_only_approval: false
active_close_approval: false
r51_strategy_evidence: false
r51_status: ACTIVE_PARTIAL_DIAGNOSTIC_ONLY
```

## Required future instrumentation

Before any upgrade, the system must produce auditable evidence for:

- exact entry quote/min_out/reserve-before/reserve-after/decimals;
- submit timestamp and submit-to-land latency;
- actual landing slot or failed/no-fill status;
- exit quote/min_out/slippage/fees/own sell impact;
- per-path sample slot/timestamp/commitment;
- exact tie-break metadata for same-slot target/stop;
- lifecycle/replay exact join id and terminal-event cardinality.

## Consequence

Until the missing instrumentation is added and re-audited, no old strategy report may be used as live-equivalent PnL proof, executable fill proof, live slippage proof, or real landing outcome proof.
