# ADR-8D: Shadow V2 Validation Harness Smoke PR16A R2

## Status

Accepted as negative smoke evidence.

## D1. Problem

Po merge PR16A należało powtórzyć logging-only smoke, żeby sprawdzić, czy Shadow V2 validation harness potrafi przejść pełną ścieżkę:

`preflight -> canonical JSONL -> derived replay/lifecycle -> density rows -> post_run_manifest PASS -> clean shutdown`

bez zależności od BUY, bez accepted shadow handoff i bez uruchamiania PR17 fidelity validation burnin.

## D2. Decyzja

Smoke r2 klasyfikujemy jako:

`FAIL_BLOCKED_SCHEMA_CONTRACT_AND_SHUTDOWN`

Nie przyznajemy:

- `runtime_approval`;
- `shadow_close_only_approval`;
- `active_close_approval`;
- `research_grade`;
- `live_equivalence`;
- zgody na PR17 fidelity validation burnin.

## D3. Kontekst

PR16 wykazał `FAIL_BLOCKED_NO_CANONICAL_V2_EVIDENCE`, ponieważ harness czekał na accepted handoff. PR16A dodał deterministic smoke marker gated przez `shadow_v2_burnin.enabled && logging_only`, emitowany przez realny `ShadowV2ValidationHarness::append_record()`.

Po merge PR16A smoke r2 potwierdził, że marker przechodzi przez realny writer/materializer:

- `shadow_position_event_v2.jsonl`: 1 row;
- `shadow_replay_v2.jsonl`: 1 row;
- `shadow_lifecycle_v2.jsonl`: 1 row;
- `shadow_path_density_v2.jsonl`: 7 rows.

## D4. Dowody

Pre-run:

- pre-run manifest generation: `PASS`;
- pre-run strict audit: `PASS`;
- launcher preflight: `PASS`;
- NLN gRPC app probe: `PASS`;
- runtime stream established: `PASS`.

Runtime:

- log evidence: `PostBuyRuntime: Shadow V2 validation smoke marker emitted`;
- log evidence: `Stream established`;
- log evidence: `PostBuyRuntime received shutdown signal; draining late PostBuySubmitted events for 10000ms`.

Post-run:

- `post_run_manifest.json`: exists;
- manifest status: `BLOCKED`;
- strict audit: `FAIL`;
- blocker: `shadow_position_event_v2.jsonl: expected schema shadow_position_event_v2 not found`.

Schema inspection:

- `shadow_position_event_v2.jsonl` top-level `schema`: missing;
- `shadow_position_event_v2.jsonl` `envelope.schema`: `shadow_position_v2`;
- `shadow_replay_v2.jsonl` `envelope.schema`: `shadow_replay_v2`;
- `shadow_lifecycle_v2.jsonl` `envelope.schema`: `shadow_lifecycle_v2`;
- `shadow_path_density_v2.jsonl` top-level `schema`: `shadow_path_density_v2`.

Shutdown:

- runtime did not exit after first SIGINT;
- runtime did not exit after second SIGINT;
- process stopped only after SIGTERM;
- session exit code: `1`;
- `clean_shutdown_proven=false`.

## D5. Root Cause

Root cause 1:

Manifest audit expects artifact `shadow_position_event_v2.jsonl` to expose schema `shadow_position_event_v2`, while canonical writer emits a canonical event wrapper whose envelope schema is `shadow_position_v2`. This is a contract mismatch between event artifact schema and payload schema.

Root cause 2:

SIGINT initiates shutdown, but the runtime does not terminate within the smoke window and continues emitting repeated `Transport channel disconnected` messages from the gRPC transport path. Clean shutdown cannot be claimed.

## D6. Konsekwencje

PR16A closed the previous no-canonical-evidence gap, but did not close the full smoke acceptance gate.

Therefore:

- PR17 fidelity validation burnin remains blocked;
- Shadow V2 remains contract/harness validation work, not strategy evidence;
- no runtime decision can consume Shadow V2 evidence;
- raw JSONL/log artifacts remain local-only evidence;
- positive smoke report cannot be issued yet.

## D7. Runtime Boundary

No runtime policy approval is granted.

The smoke was launched in shadow/logging-only mode:

- no BUY/REJECT change;
- no Gatekeeper policy change;
- no selector runtime change;
- no TX/Jito/live path change;
- no shadow_close_only enablement;
- no active close enablement.

## D8. Required Follow-Up

Before PR17:

1. Add PR16B/PR15-fix for the `shadow_position_event_v2` schema contract:
   - define whether the canonical event wrapper has top-level schema `shadow_position_event_v2`, or
   - update manifest audit/schema manifest to treat payload `shadow_position_v2` inside canonical event artifact as valid.

2. Add shutdown fix:
   - SIGINT must complete logging-only shutdown;
   - post-run manifest generation and strict verification must complete before exit;
   - gRPC disconnect loop must not keep the process alive.

3. Repeat smoke and require:
   - canonical rows > 0;
   - replay rows > 0;
   - lifecycle rows > 0;
   - density rows > 0;
   - post-run manifest `PASS`;
   - post-run strict audit `PASS`;
   - clean shutdown proven.
