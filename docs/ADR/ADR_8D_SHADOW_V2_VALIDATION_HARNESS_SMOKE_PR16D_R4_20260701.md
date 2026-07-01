# ADR-8D: Shadow V2 Validation Harness Smoke PR16D R4

## Status

Accepted as negative smoke evidence.

## D1. Problem

Po merge PR16C nalezalo powtorzyc logging-only smoke i sprawdzic pelna bramke:

`preflight -> canonical JSONL -> derived replay/lifecycle -> density rows -> post_run_manifest PASS -> clean shutdown`

Smoke r4 spelnil bramki evidence/manifest oraz potwierdzil, ze Seer/gRPC transport loop nie flooduje po shutdownie. Nie spelnil jednak pelnej bramki clean shutdown, poniewaz launcher nie zalogowal koncowego zamkniecia po `Waiting for Watchdog to shut down...`.

## D2. Decyzja

Smoke r4 klasyfikujemy jako:

`FAIL_BLOCKED_LAUNCHER_WATCHDOG_RECONCILIATION_SHUTDOWN_WAIT`

Nie przyznajemy:

- `runtime_approval`;
- `shadow_close_only_approval`;
- `active_close_approval`;
- `research_grade`;
- `live_equivalence`;
- zgody na PR17 fidelity validation burnin.

## D3. Kontekst

R3 failowal przez Seer/gRPC transport loop po shutdownie i wymagal SIGTERM.

PR16C mial naprawic tylko ten wąski problem: po globalnym shutdown Seer/gRPC nie moze kontynuowac reconnect/read/subscribe loop i floodowac `Transport channel disconnected`.

Smoke r4 mial potwierdzic, czy po PR16C caly proces konczy sie czysto i zostawia PASS manifest.

## D4. Dowody

Baseline:

- PR #17 head:
  `eaf9cc91de83652550d36400b372aac2163f775a`;
- lokalny `main` po merge PR #17:
  `5359a6c2e1622823fc09d7b2f1506fff3360d21d`.

Pre-run:

- pre-run manifest generation: `PASS`;
- pre-run strict audit: `PASS`;
- launcher preflight: `PASS`;
- NLN gRPC app probe: `PASS`;
- runtime stream established: `PASS`.

Shadow V2 evidence:

- `shadow_position_event_v2.jsonl`: `1` row;
- `shadow_replay_v2.jsonl`: `1` row;
- `shadow_lifecycle_v2.jsonl`: `1` row;
- `shadow_path_density_v2.jsonl`: `7` rows.

Manifest:

- `post_run_manifest.status=PASS`;
- blockers: `[]`;
- post-run strict audit: `PASS`.

Static guards:

- `python3 scripts/shadow_v2_validation_burnin_plan_audit.py --strict`: `PASS`;
- `python3 scripts/shadow_v2_legacy_downgrade_audit.py --strict`: `PASS`.

Shutdown evidence:

- `Transport channel disconnected`: `0`;
- `SIGTERM`: `0`;
- `Seer: Component stopped`: `1`;
- `Seer shut down successfully`: `1`;
- `PostBuyRuntime: Shadow V2 post-run manifest generated and strict-verified`: `1`;
- `PostBuyRuntime shut down successfully`: `1`;
- `Oracle Runtime shut down successfully`: `1`;
- `Watchdog shut down successfully`: `0`;
- `All components shut down`: `0`;
- `clean_shutdown_proven=false`.

## D5. Root Cause

PR16C fixed the Seer/gRPC shutdown loop. The new residual blocker is not `Transport channel disconnected` flood.

Observed terminal sequence:

1. Seer receives shutdown and stops its core event loop.
2. PostBuyRuntime drains, generates post-run manifest and strict-verifies it.
3. Launcher logs successful shutdown for PostBuyRuntime, Seer, Trigger, GUI Backend, SnapshotListener, GatekeeperCommitLoop and LivePipelineFlushLoop.
4. Launcher reaches `Waiting for Watchdog to shut down...`.
5. Later log lines still contain `WATCHDOG | grpc_state=DISCONNECTED reconnects=0` and `ReconciliationRuntime health`.
6. No final clean-shutdown line appears in the captured log.

This is classified as:

`LAUNCHER_WATCHDOG_RECONCILIATION_SHUTDOWN_WAIT`

## D6. Konsekwencje

Positive smoke PASS cannot be issued.

PR17 fidelity validation burnin remains blocked because clean shutdown is part of the harness readiness gate. A run that produces all evidence artifacts and PASS manifest still cannot be treated as completed harness proof without an auditable clean process exit.

## D7. Runtime Boundary

The smoke used logging-only Shadow V2 validation mode.

No runtime approval is granted.

No changes or approvals are made for:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close;
- RCE;
- strategy research.

## D8. Required Follow-Up

Before PR17:

1. Add PR16E / focused shutdown fix for Watchdog/Reconciliation/final launcher join.
2. Ensure global shutdown signal stops the residual periodic health loop.
3. Ensure `Waiting for Watchdog to shut down...` reaches success or a typed bounded failure.
4. Repeat logging-only smoke.
5. Require:
   - canonical rows > 0;
   - replay rows > 0;
   - lifecycle rows > 0;
   - density rows > 0;
   - `post_run_manifest.status=PASS`;
   - post-run strict audit PASS;
   - `clean_shutdown_proven=true`;
   - no SIGTERM;
   - no reconnect/disconnect flood after shutdown.
