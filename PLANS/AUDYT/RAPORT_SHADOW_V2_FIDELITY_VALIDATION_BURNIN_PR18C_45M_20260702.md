# RAPORT SHADOW V2 FIDELITY VALIDATION BURNIN PR18C 45M 20260702

## 1. Werdykt wykonawczy

Werdykt runu:

`PASS`

Run `shadow-burnin-v2-fidelity-validation-pr18c-45m-r1` spelnil operacyjne bramki PR18C/PR27 dla kontrolowanego validation/fidelity-only burnina:

- runtime `post_run_manifest.status=PASS`;
- post-run strict manifest audit: `PASS`;
- `PostBuyRuntime` zakonczyl sie bez timeoutu i bez abortu;
- launcher zakonczyl sie cleanly po SIGINT;
- `All components shut down successfully`: obecne;
- `Ghost Launcher shutdown complete`: obecne;
- `clean_shutdown_proven=true`;
- canonical V2 event families sa obecne;
- `real_shadow_v2_positions=129`, czyli `>50`;
- malformed canonical rows: `0`.

To nie nadaje automatycznie `research_grade`, `runtime_approval`, `shadow_close_only_approval`, `active_close_approval` ani `live_equivalence`. Ten run potwierdza, ze po PR27 runtime potrafi zapisac, zflushowac, zmanifestowac i strict-zwalidowac Shadow V2 evidence w kontrolowanym shutdownie.

Najwazniejsze ograniczenie: `shadow_entry_fill_v2` i `shadow_exit_fill_v2` rows istnieja, ale wszystkie sa `BLOCKED_BY_DATA`. To oznacza, ze rodziny eventow sa emitowane i audytowalne, ale ten run nie dowodzi executable live fill, slippage, landing, no-fill/failure modelu ani live-equivalence.

## 2. Zakres i granice

Zakres wykonany:

- zmergowano PR27 / PR18C do `main`;
- uruchomiono jeden kontrolowany 45-minutowy validation/fidelity-only burnin;
- uzyto lokalnego relaxed validation sampling profile;
- wygenerowano runtime Shadow V2 evidence;
- wykonano post-run manifest strict audit;
- przygotowano report-only PR bez raw evidence.

Granice nienaruszone:

- brak zmian BUY/REJECT;
- brak zmian Gatekeeper policy code;
- brak zmian selector runtime;
- brak zmian TX/Jito/live path;
- `shadow_close_only=false`;
- active close disabled;
- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- brak strategy proof;
- brak RCE proof;
- brak selector/edge proof;
- brak live-equivalence claim;
- brak R51;
- raw JSONL/log/runtime artifacts nie sa commitowane.

## 3. PR27 i baseline main

PR27:

- PR head przed merge: `000c81def0929bc65e9d18ee16df59b6966a5d35`;
- merge commit PR27: `c44dfc321a72faa34828f81e7fb4aaa8a7fc3422`;
- lokalny `main` po `git pull --ff-only`: `c44dfc321a72faa34828f81e7fb4aaa8a7fc3422`.

CI PR27 przed merge:

- Level 1 static restore guard: `SUCCESS`;
- Level 2 runtime restore guard: `SKIPPED`.

## 4. Run configuration

Run id:

`shadow-burnin-v2-fidelity-validation-pr18c-45m-r1`

Scope root:

`reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1`

Lokalne, niestage'owane configi:

- `configs/rollout/shadow-v2-fidelity-validation-pr18c-45m-r1.local.toml`;
- `configs/rollout/ghost_brain_shadow_v2_fidelity_validation_pr18c_45m_r1.local.toml`.

Minimalny profil operatora:

| Field | Value |
|---|---:|
| `min_tx_count` | `5` |
| `min_buy_count` | `3` |
| `min_unique_signers` | `3` |
| `min_market_cap_sol` | `5.0` |

Zachowane approval flags:

| Flag | Value |
|---|---|
| `shadow_v2_burnin.enabled` | `true` |
| `shadow_v2_burnin.logging_only` | `true` |
| `runtime_approval` | `false` |
| `shadow_close_only_approval` | `false` |
| `active_close_approval` | `false` |
| `post_run_manifest_drain_timeout_ms` | `180000` |

Launcher preflight:

- status: `PASS`;
- execution mode: `Shadow`;
- entry mode: `shadow_only`;
- Ghost Brain thresholds observed: `min_tx=5`, `min_unique=3`, `min_buy=3`, `max_wait_ms=10000`;
- Spectrum RPC probe: `PASS`;
- NLN gRPC app probe: `PASS`.

## 5. Pre-run gates

Pre-run manifest generation:

- status: `PASS`;
- blockers: `[]`;
- artifact_count before run: `0`.

Pre-run strict manifest audit:

- status: `PASS`;
- blockers: `[]`;
- artifact_count: `2`;
- total_size_bytes: `812`.

Validation burnin plan audit:

- command: `python3 scripts/shadow_v2_validation_burnin_plan_audit.py --strict`;
- status: `PASS`;
- `validation_mode=FIDELITY_ONLY`;
- `runtime_approval=false`;
- `strategy_proof_enabled=false`;
- blockers: `[]`.

Legacy downgrade audit:

- command: `python3 scripts/shadow_v2_legacy_downgrade_audit.py --strict`;
- status: `PASS`;
- `v1_live_equivalent_allowed=false`;
- blockers: `[]`.

Build/preflight:

- `cargo build -p ghost-launcher --bin ghost-launcher --release`: `PASS`;
- launcher `--preflight`: `PASS`.

## 6. Runtime i shutdown

Run start:

`2026-07-02T10:50:10Z`

SIGINT sent:

`2026-07-02T11:40:01Z`

Final shutdown:

`2026-07-02T11:40:35.459Z`

Elapsed runtime:

- process runtime to SIGINT: about `49m51s`;
- process runtime to final shutdown: about `50m25s`.

Shutdown method:

`SIGINT`

Controller exit status:

`0`

Shutdown evidence:

- `PostBuyRuntime: Shadow V2 post-run manifest generated and strict-verified`: present;
- `PostBuyRuntime shut down successfully`: present;
- `Seer shut down successfully`: present;
- `Trigger shut down successfully`: present;
- `SnapshotListener shut down successfully`: present;
- `GatekeeperCommitLoop shut down successfully`: present;
- `LivePipelineFlushLoop shut down successfully`: present;
- `Watchdog shut down successfully`: present;
- `All components shut down successfully`: present;
- `Ghost Launcher shutdown complete`: present.

Negative shutdown checks:

- `SIGTERM`: not used;
- `SHADOW_V2_POST_RUN_MANIFEST_DRAIN_TIMEOUT`: not observed;
- PostBuyRuntime shutdown join timeout: not observed;
- forced component abort: not observed;
- reconnect/disconnect flood after shutdown: not observed.

## 7. Runtime post-run manifest

Runtime-written manifest:

`reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1/post_run_manifest.json`

Status:

- `post_run_manifest.status=PASS`;
- blockers: `[]`;
- created_at: `2026-07-02T11:40:35+00:00`;
- artifact_count: `6`;
- total_size_bytes: `25059202`.

Schema coverage from runtime manifest:

| Schema | Rows |
|---|---:|
| `shadow_position_event_v2` | `1022` |
| `shadow_replay_v2` | `1022` |
| `shadow_lifecycle_v2` | `1022` |
| `shadow_path_density_v2` | `7154` |

Post-run strict audit:

- command: `python3 scripts/shadow_v2_manifest_audit.py --scope-root reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1 --manifest-phase post_run --schema-manifest reports/selector/shadow_v2_required_schema_manifest.csv --acceptance-gates reports/selector/shadow_v2_acceptance_gates.csv --strict`;
- status: `PASS`;
- blockers: `[]`;
- strict scan artifact_count: `7`;
- strict scan total_size_bytes: `25063619`.

## 8. Canonical V2 evidence

Canonical artifact:

`reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1/shadow_position_event_v2.jsonl`

Canonical row summary:

| Metric | Value |
|---|---:|
| canonical rows | `1022` |
| malformed canonical rows | `0` |
| unique positions total | `130` |
| diagnostic smoke positions | `1` |
| real_shadow_v2_positions | `129` |

Counts per canonical payload schema:

| canonical payload schema | Rows |
|---|---:|
| `shadow_position_v2` | `130` |
| `shadow_entry_attempt_v2` | `127` |
| `shadow_entry_fill_v2` | `127` |
| `shadow_path_sample_v2` | `255` |
| `shadow_exit_attempt_v2` | `127` |
| `shadow_exit_fill_v2` | `127` |
| `shadow_terminal_truth_v2` | `129` |

Quality by canonical payload schema:

| Schema | Quality | Rows |
|---|---|---:|
| `shadow_position_v2` | `VALIDATION_HARNESS_POSITION_CREATED` | `129` |
| `shadow_position_v2` | `VALIDATION_SMOKE_MARKER_BLOCKED_BY_DATA` | `1` |
| `shadow_entry_attempt_v2` | `ENTRY_ATTEMPT_FROM_POST_BUY_HANDOFF` | `127` |
| `shadow_entry_fill_v2` | `BLOCKED_BY_DATA` | `127` |
| `shadow_path_sample_v2` | `LEGACY_LIFECYCLE_MARK_PATH_SAMPLE` | `253` |
| `shadow_path_sample_v2` | `BLOCKED_BY_DATA` | `2` |
| `shadow_exit_attempt_v2` | `DIAGNOSTIC_ONLY` | `127` |
| `shadow_exit_fill_v2` | `BLOCKED_BY_DATA` | `127` |
| `shadow_terminal_truth_v2` | `TERMINAL_TRUTH_DERIVED_FROM_LEGACY_LIFECYCLE` | `129` |

Interpretacja:

- wymagana obecnosciowa bramka family rows jest spelniona;
- `real_shadow_v2_positions > 50` jest spelnione;
- terminal truth rows sa obecne;
- path sample rows sa obecne;
- fill rows sa obecne, ale sa `BLOCKED_BY_DATA`, wiec nie stanowia executable fill proof.

## 9. Derived replay, lifecycle i density

Derived replay:

| Metric | Value |
|---|---:|
| `shadow_replay_v2.jsonl` rows | `1022` |
| malformed rows | `0` |
| real rows | `1021` |
| smoke rows | `1` |
| `REPLAY_DERIVED_FROM_CANONICAL_TERMINAL` | `129` |
| `REPLAY_DERIVED_OPEN_OR_BLOCKED` | `893` |

Derived lifecycle:

| Metric | Value |
|---|---:|
| `shadow_lifecycle_v2.jsonl` rows | `1022` |
| malformed rows | `0` |
| real rows | `1021` |
| smoke rows | `1` |
| `LIFECYCLE_DERIVED_FROM_CANONICAL_TERMINAL` | `129` |
| `LIFECYCLE_DERIVED_OPEN_OR_BLOCKED` | `893` |

Path density:

| Metric | Value |
|---|---:|
| `shadow_path_density_v2.jsonl` rows | `7154` |
| malformed rows | `0` |
| real rows | `7147` |
| smoke rows | `7` |
| `NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY` | `4438` |
| `NOT_EVALUABLE_NO_COVERAGE` | `2716` |

Interpretacja density:

- density rows sa obecne i audytowalne;
- w tym runie density verdicts pozostaja nieewaluowalne dla horyzontow bez pokrycia albo przekraczajacych replay horizon;
- nie wolno z tego runu wyciagac wnioskow o jakosci 300s/500s ani live-equivalence.

## 10. Approval flags i decyzje

Nie przyznano:

| Approval / grade | Value |
|---|---|
| `research_grade` | `false` |
| `live_equivalence` | `false` |
| `runtime_approval` | `false` |
| `shadow_close_only_approval` | `false` |
| `active_close_approval` | `false` |
| `strategy_research_unblocked` | `false` |

Poniewaz wszystkie bramki PR18C przeszly, raport moze zaproponowac wylacznie kandydatury ograniczone:

| Candidate | Value |
|---|---|
| `runtime_approval_candidate` | `true_for_shadow_v2_logging_validation_only` |
| `strategy_research_unblocked_candidate` | `true_for_offline_reconstruction_only` |

Te kandydatury nie sa approvalami. Wymagaja osobnej decyzji operatora.

## 11. Co zostalo udowodnione

Udowodnione:

- PR27 manifest drain/shutdown fix dziala w runtime burnin;
- runtime potrafi wygenerowac post-run manifest przed zakonczeniem launcher component join;
- strict post-run audit przechodzi;
- `PostBuyRuntime` nie zostal abortowany;
- launcher zakonczyl wszystkie komponenty cleanly;
- canonical Shadow V2 writer emituje wymagane event family rows;
- derived replay/lifecycle/density powstaja z canonical evidence;
- `real_shadow_v2_positions=129`;
- canonical malformed rows = `0`;
- raw evidence nie musi byc commitowane do report-only PR.

## 12. Czego nie udowodniono

Nieudowodnione:

- executable live entry fill;
- executable live exit fill;
- live slippage;
- live landing outcome;
- failed transaction/no-fill model;
- quote/fill divergence calibration;
- live-equivalence;
- research-grade reconstruction correctness;
- density evaluability dla horyzontow bez pokrycia;
- strategy edge;
- RCE proof;
- runtime approval.

## 13. Final decision

Final verdict:

`PASS`

Status po runie:

- Shadow V2 logging validation path: `PASS`;
- Shadow V2 evidence emission: `PASS_WITH_BLOCKED_FILL_LIMITATIONS`;
- runtime post-run manifest flush: `PASS`;
- clean shutdown: `PASS`;
- research-grade: `NOT_GRANTED`;
- live-equivalence: `NOT_GRANTED`;
- runtime approval: `NOT_GRANTED`;
- shadow_close_only approval: `NOT_GRANTED`;
- active close approval: `NOT_GRANTED`;
- PR17/strategy proof: nadal wymaga osobnej decyzji i osobnych offline reconstruction/reconciliation auditow.
