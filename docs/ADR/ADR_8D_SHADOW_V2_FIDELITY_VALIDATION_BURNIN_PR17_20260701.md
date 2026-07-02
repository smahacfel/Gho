# ADR-8D: Shadow V2 Fidelity Validation Burnin PR17

## Status

Accepted as blocked fidelity validation evidence.

## D1. Problem

Po pozytywnym smoke PR16F nalezalo wykonac pierwszy PR17 Shadow V2 fidelity validation burnin.

Cel PR17 nie byl strategia, edge proof ani runtime approval. Cel byl ograniczony do sprawdzenia, czy logging-only Shadow V2 validation run potrafi wygenerowac realne evidence potrzebne do pozniejszej walidacji:

- canonical `shadow_position_event_v2.jsonl`;
- derived `shadow_replay_v2.jsonl`;
- derived `shadow_lifecycle_v2.jsonl`;
- `shadow_path_density_v2.jsonl`;
- pre-run i post-run manifesty;
- clean shutdown;
- material do rekonstrukcji entry/exit/path tam, gdzie dane istnieja.

## D2. Decyzja

Klasyfikujemy PR17 r1 jako:

`BLOCKED_NO_REAL_SHADOW_V2_POSITION_EVIDENCE`

Run technicznie przeszedl bramki infrastrukturalne:

- pre-run manifest: `PASS`;
- pre-run strict audit: `PASS`;
- launcher preflight: `PASS`;
- post-run manifest: `PASS`;
- post-run strict audit: `PASS`;
- clean shutdown: `PASS`;
- `SIGTERM=false`;
- reconnect/disconnect flood: `false`.

Nie przeszedl bramki fidelity validation, bo wygenerowal tylko diagnostyczny `VALIDATION_SMOKE_MARKER`, a nie realna pozycje z entry/fill/path/exit.

Nie przyznajemy:

- `runtime_approval`;
- `shadow_close_only_approval`;
- `active_close_approval`;
- `research_grade`;
- `live_equivalence`;
- strategy research unblocked.

## D3. Kontekst

Baseline:

`286a1f76f5c5fd632800f60afa6b4be98066eec7`

Run id:

`shadow-burnin-v2-fidelity-validation-pr17-r1`

Scope root:

`reports/selector/shadow-v2-fidelity-validation-pr17-r1`

Runtime byl uruchomiony z lokalnych, niestage'owanych configow `*.local.toml`.

Konfiguracja zachowala:

- `shadow_v2_burnin.enabled=true`;
- `shadow_v2_burnin.logging_only=true`;
- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `execution_mode=shadow`;
- `entry_mode=shadow_only`;
- NLN gRPC + Program Streams;
- Spectrum RPC.

## D4. Dowody

Pre-run:

- `pre_run_manifest.status=PASS`;
- blockers: `[]`;
- `python3 scripts/shadow_v2_manifest_audit.py ... --strict`: `PASS`;
- `python3 scripts/shadow_v2_validation_burnin_plan_audit.py --strict`: `PASS`;
- `python3 scripts/shadow_v2_legacy_downgrade_audit.py --strict`: `PASS`;
- `cargo build -p ghost-launcher --bin ghost-launcher --release`: `PASS`;
- launcher `--preflight`: `PASS`.

Runtime:

- run start: `2026-07-01T23:18:35.652Z`;
- shutdown signal: `2026-07-01T23:24:38.679Z`;
- final shutdown: `2026-07-01T23:24:48.840Z`;
- `All components started successfully`: `1`;
- `NewPoolDetected`: `420`;
- `Detected new pool`: `210`;
- `Runtime Shadow Buy Submitted`: `0`;
- `Shadow V2 validation smoke marker emitted`: `1`;
- `Transport channel disconnected`: `0`;
- `NLN Subscribe request failed`: `0`;
- `SIGTERM`: `0`;
- process exit code: `0`.

Shadow V2 artifacts:

- `shadow_position_event_v2.jsonl`: `1` row;
- `shadow_replay_v2.jsonl`: `1` row;
- `shadow_lifecycle_v2.jsonl`: `1` row;
- `shadow_path_density_v2.jsonl`: `7` rows;
- all density verdicts: `NOT_EVALUABLE_NO_COVERAGE`;
- `post_run_manifest.status=PASS`;
- post-run strict audit: `PASS`.

Canonical evidence classification:

- `candidate_id=VALIDATION_SMOKE_MARKER`;
- `simulation_level=MARK_ONLY`;
- `measurement_grade=DIAGNOSTIC_ONLY`;
- `temporal_class=UNKNOWN`;
- `quality=VALIDATION_SMOKE_MARKER_BLOCKED_BY_DATA`;
- `NOT_STRATEGY_EVIDENCE`;
- `NOT_LIVE_EQUIVALENT`;
- `NO_BUY_REJECT_CHANGE`.

Decision logs pozostaja lokalnym runtime artifact:

- gatekeeper decision rows: `286`;
- malformed rows: `0`;
- `decision_verdict_buy=true`: `0`.

## D5. Root Cause Classification

Root cause blockera:

`NO_REAL_ACCEPTED_SHADOW_HANDOFF_DURING_PR17_WINDOW`

W aktualnym oknie Gatekeeper/decision runtime nie wygenerowal realnej pozycji, ktora przeszlaby do Shadow V2 validation harness jako entry/fill/path candidate. Harness zapisal tylko marker diagnostyczny emitowany na starcie.

To potwierdza, ze:

- PR17 r1 ma sprawna infrastrukture evidence;
- PR17 r1 nie ma materialu do fidelity reconstruction.

Nie jest to dowod, ze Shadow V2 entry/exit fidelity dziala lub nie dziala. Jest to brak wymaganej probki realnej pozycji.

## D6. Konsekwencje

Blokady:

- `ENTRY_PRICE_FIDELITY=BLOCKED_NO_REAL_ENTRY_FILL`;
- `EXIT_PRICE_FIDELITY=BLOCKED_NO_REAL_EXIT_FILL`;
- `REPLAY_LIFECYCLE_FIDELITY=BLOCKED_MARKER_ONLY`;
- `TEMPORAL_NO_LOOKAHEAD=BLOCKED_NO_REAL_POSITION_FIELDS`;
- `PATH_DENSITY=NOT_EVALUABLE_NO_COVERAGE`;
- `RECONSTRUCTION_READY=false`.

Nie wolno uzyc PR17 r1 jako dowodu:

- live PnL;
- executable fills;
- slippage behavior;
- landing outcome;
- active close readiness;
- shadow_close_only readiness;
- RCE;
- selector edge;
- runtime promotion.

## D7. Runtime Boundary

PR17 r1 nie zmienil runtime semantics.

Nie zmieniono:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close.

Nie dotykano R51.

Raw JSONL/log/runtime artifacts pozostaja lokalne i nie sa commitowane.

## D8. Required Follow-Up

PR17 r1 wymaga follow-up przed jakimkolwiek research-grade claim:

1. Operator musi zdecydowac, czy powtorzyc validation burnin dluzej przy tej samej polityce, czy stworzyc osobny diagnostic-only producer realnych Shadow V2 candidate records.
2. Kazde rozwiazanie musi zachowac brak konsumpcji Shadow V2 przez Gatekeeper, selector, BUY/REJECT i TX path.
3. Nastepny run musi wygenerowac realne rekordy entry/fill/path/exit albo jawnie oznaczone `BLOCKED_BY_DATA` real-candidate records, inaczej fidelity reconstruction pozostanie zablokowana.
4. Stare downgrade labels pozostaja w mocy do czasu realnej rekonstrukcji Shadow V2.
