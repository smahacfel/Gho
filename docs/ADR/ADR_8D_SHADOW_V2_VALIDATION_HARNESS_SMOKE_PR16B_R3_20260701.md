# ADR-8D: Shadow V2 Validation Harness Smoke PR16B R3

## Status

Accepted as negative smoke evidence.

## D1. Problem

Po merge PR16B należało powtórzyć logging-only smoke i sprawdzić pełną bramkę:

`preflight -> canonical JSONL -> derived replay/lifecycle -> density rows -> post_run_manifest PASS -> clean shutdown`

Smoke r3 spełnił bramki evidence/manifest, ale nie spełnił clean shutdown.

## D2. Decyzja

Smoke r3 klasyfikujemy jako:

`FAIL_BLOCKED_CLEAN_SHUTDOWN_SEER_TRANSPORT_LOOP`

Nie przyznajemy:

- `runtime_approval`;
- `shadow_close_only_approval`;
- `active_close_approval`;
- `research_grade`;
- `live_equivalence`;
- zgody na PR17 fidelity validation burnin.

## D3. Kontekst

PR16A wprowadził deterministic smoke marker i zamknął zależność od losowego BUY/handoff.

PR16B naprawił:

- top-level schema dla canonical event artifact;
- OracleRuntime shutdown przez globalny shutdown receiver.

Smoke r3 miał potwierdzić, czy po tych poprawkach cały proces kończy się czysto i zostawia PASS manifest.

## D4. Dowody

Baseline:

- `main` po merge PR16B:
  `29032341089a28217035cc6f6d56594788aa02c7`.

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

Schema:

- top-level `schema=shadow_position_event_v2`;
- `canonical_payload_schema=shadow_position_v2`.

Manifest:

- `post_run_manifest.status=PASS`;
- blockers: `[]`;
- post-run strict audit: `PASS`.

Shutdown:

- first SIGINT did not terminate process;
- second SIGINT did not terminate process;
- forced stop: `SIGTERM`;
- `clean_shutdown_proven=false`;
- `Transport channel disconnected` count: `29034`.

## D5. Root Cause

PR16B fixed OracleRuntime shutdown: log evidence includes `Oracle Runtime shut down successfully`.

Remaining blocker is outside the canonical writer and OracleRuntime path. After global shutdown signal, Seer receives shutdown but gRPC transport continues emitting `Transport channel disconnected`, and the process does not exit.

This is classified as:

`SEER_GRPC_SHUTDOWN_LOOP`

## D6. Konsekwencje

Positive smoke PASS cannot be issued.

PR17 fidelity validation burnin remains blocked because clean shutdown is part of the harness readiness gate. A run that needs SIGTERM cannot be considered a completed validation harness proof, even if manifest artifacts are correct.

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

1. Add PR16C / shutdown fix for Seer/gRPC.
2. Ensure global shutdown signal cancels receive/reconnect/transport loops.
3. Ensure process exits after first SIGINT without SIGTERM.
4. Repeat logging-only smoke.
5. Require:
   - canonical rows > 0;
   - replay rows > 0;
   - lifecycle rows > 0;
   - density rows > 0;
   - `post_run_manifest.status=PASS`;
   - post-run strict audit PASS;
   - clean shutdown proven.
