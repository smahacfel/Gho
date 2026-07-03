# ADR-8D: Shadow V2 Terminal Executable PnL Smoke PR41

Data: 2026-07-03

## Status

```text
ACCEPTED_AS_DIAGNOSTIC_TERMINAL_EXECUTABLE_PNL_SMOKE_EVIDENCE
```

Final smoke verdict:

```text
PR41_TERMINAL_EXECUTABLE_PNL_SMOKE_PASS
```

## D1. Context

PR41 dodal code-level wiring dla `shadow_terminal_truth_v2.final_pnl_executable_bps`, ale sama implementacja wymagala runtime smoke. Cel smoke byl waski: sprawdzic, czy realny shadow flow po merge PR41 tworzy terminal truth z executable PnL oraz exact links do canonical FILLED entry/exit fills dla tego samego `position_id`.

Ten smoke nie byl strategy proof, edge proof, runtime approval, research-grade ani live-equivalence.

## D2. Decision

Wykonano jeden operator-approved smoke:

```text
run_id = shadow-smoke-v2-terminal-executable-pnl-pr41-r1
scope = reports/selector/shadow-v2-terminal-executable-pnl-pr41-r1
main_head = b68ec301e3b17a3a0a81ef005fb1cf37083bc421
PR41_merge_commit = b68ec301e3b17a3a0a81ef005fb1cf37083bc421
configured_run_seconds = 900
duration_seconds = 932
shutdown = SIGINT_CLEAN
```

Decyzja: zaakceptowac wynik jako diagnostic terminal executable PnL smoke evidence.

## D3. Evidence

Runtime gates:

- `cargo build -p ghost-launcher --release`: PASS.
- pre-run manifest generation: PASS.
- pre-run strict manifest audit: PASS.
- validation burnin plan audit: PASS / `FIDELITY_ONLY`.
- legacy downgrade audit: PASS.
- runtime post-run manifest: PASS.
- post-run strict audit: PASS.
- clean shutdown: PASS.
- forced SIGTERM: false.
- forced component abort: false.

Canonical counters:

| Metric | Value |
|---|---:|
| accepted shadow handoffs | 28 |
| entry fills FILLED | 28 |
| exit fills FILLED | 28 |
| entry+exit FILLED same position | 28 |
| terminal truth rows | 28 |
| terminal truth with executable PnL | 28 |
| complete executable roundtrip positions | 28 |
| exact terminal entry+exit link pairs | 28 |
| LIVE_CONFIRMED exit fills | 0 |

Sample exact linked terminal:

```text
position_id = CVzzi42CiPoY8L32fTuCyvcrssfx4wTTh5RyMuK7UgGS:FoUkPTEhD8NyaCPkCTWsR74TxiYcpSoNzLwBMGcEpump:1783101543594
terminal_truth_event_id = shadow_v2_terminal_truth:CVzzi42CiPoY8L32fTuCyvcrssfx4wTTh5RyMuK7UgGS:FoUkPTEhD8NyaCPkCTWsR74TxiYcpSoNzLwBMGcEpump:1783101543594:1783101543956:STOP
linked_entry_fill_event_id = shadow_v2_entry_fill:CVzzi42CiPoY8L32fTuCyvcrssfx4wTTh5RyMuK7UgGS:FoUkPTEhD8NyaCPkCTWsR74TxiYcpSoNzLwBMGcEpump:1783101543594:1783101536771
linked_exit_fill_event_id = shadow_v2_exit_fill:CVzzi42CiPoY8L32fTuCyvcrssfx4wTTh5RyMuK7UgGS:FoUkPTEhD8NyaCPkCTWsR74TxiYcpSoNzLwBMGcEpump:1783101543594:1783101543887:exit_filled
entry_fill_status = FILLED
exit_fill_status = FILLED
final_pnl_executable_bps = -5486
```

## D4. Audit Results

| Audit | Verdict |
|---|---|
| entry reconstruction readiness | `PASS_ENTRY_RECONSTRUCTION_READY` |
| exit reconstruction readiness | `PASS_EXIT_RECONSTRUCTION_READY` |
| replay/lifecycle reconciliation | `PASS_REPLAY_LIFECYCLE_RECONCILED` |
| temporal/no-lookahead | `BLOCKED_TEMPORAL_AMBIGUITY_REMAINS` |
| manifest retention | `PASS_MANIFEST_RETENTION_AUDIT` |
| path density horizon | `BLOCKED_DENSITY_NOT_EVALUABLE_FOR_REQUIRED_HORIZONS` |

Temporal audit remains blocked by explicit UNKNOWN chain-order ambiguity. Path density remains not evaluable for required horizons. These are still research-grade blockers.

## D5. Invariants Preserved

The smoke did not change or approve:

```text
BUY/REJECT = unchanged
Gatekeeper policy = unchanged
selector runtime = unchanged
TX/Jito/live path = unchanged
R51 = untouched
shadow_close_only = disabled / not approved
active close = disabled / not approved
runtime_approval = false
research_grade = false
live_equivalence = false
strategy_research_unblocked = false
```

## D6. Rejected Interpretations

Rejected:

- treating diagnostic executable PnL as live PnL;
- treating diagnostic fills as landed live fills;
- claiming realized slippage or quote/fill divergence;
- claiming research-grade while temporal/order and density audits remain blocked;
- claiming strategy profitability or edge;
- enabling runtime, shadow close-only, or active close approval.

## D7. Consequences

1. PR41 runtime smoke confirms terminal executable PnL wiring works in real shadow flow.
2. `PLAN_PR36_L1_DETERMINISTIC_EXECUTION_SIM_READY_CANDIDATE` can be recorded as candidate status for L1 deterministic execution simulation only.
3. Research-grade remains blocked until temporal/order ambiguity, density evaluability, and provenance requirements are addressed.
4. Live-equivalence remains blocked until live-confirmed calibration and real fill telemetry exist.
5. No strategy research is unblocked automatically by this smoke.

## D8. Final Flags

```text
runtime_approval = false
shadow_close_only_approval = false
active_close_approval = false
research_grade = NOT_GRANTED
live_equivalence = NOT_GRANTED
strategy_research_unblocked = false
edge_proven = false
```
