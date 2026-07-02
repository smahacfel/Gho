# RAPORT SHADOW V2 OFFLINE RECONSTRUCTION AUDITS PR18D 20260702

## 1. Werdykt wykonawczy

Finalny werdykt PR18D:

`BLOCKED_EXECUTABLE_FILL_PROVENANCE_MISSING`

Offline audyty zostaly wykonane na istniejacym lokalnym scope z PR18C 45m:

`reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1`

Nie uruchomiono runtime. Nie uruchomiono burnina. Nie zmieniono runtime behavior.

Wynik najkrotszy:

- manifest/retention: `PASS_MANIFEST_RETENTION_AUDIT`;
- replay/lifecycle terminal reconciliation: `PASS_REPLAY_LIFECYCLE_RECONCILED`;
- entry reconstruction readiness: `BLOCKED_ENTRY_FILLS_BLOCKED_BY_DATA`;
- exit reconstruction readiness: `BLOCKED_EXIT_FILLS_BLOCKED_BY_DATA`;
- path density: `BLOCKED_DENSITY_NOT_EVALUABLE_FOR_REQUIRED_HORIZONS`;
- temporal/no-lookahead: `BLOCKED_TEMPORAL_AMBIGUITY_REMAINS`.

To oznacza, ze Shadow V2 PR18C evidence jest spójne jako canonical/derived logging surface i ma reconciled terminal snapshots, ale nie jest jeszcze gotowe do executable fill reconstruction ani research-grade conclusions. Glowny bloker to brak pool-state provenance i fill telemetry w `shadow_entry_fill_v2` oraz `shadow_exit_fill_v2`.

## 2. Scope i input artifacts

Audyt czytal wylacznie lokalne artefakty ze scope:

- `shadow_position_event_v2.jsonl`;
- `shadow_replay_v2.jsonl`;
- `shadow_lifecycle_v2.jsonl`;
- `shadow_path_density_v2.jsonl`;
- `pre_run_manifest.json`;
- `post_run_manifest.json`;
- `shadow_v2_manifest_report.csv`.

Raw JSONL i runtime scope nie sa commitowane.

## 3. Dodane offline audit scripts

Dodano deterministyczne skrypty offline:

- `scripts/shadow_v2_offline_audit_common.py`;
- `scripts/shadow_v2_entry_reconstruction_readiness_audit.py`;
- `scripts/shadow_v2_exit_reconstruction_readiness_audit.py`;
- `scripts/shadow_v2_replay_lifecycle_terminal_reconciliation_audit.py`;
- `scripts/shadow_v2_path_density_horizon_audit.py`;
- `scripts/shadow_v2_temporal_no_lookahead_audit.py`;
- `scripts/shadow_v2_manifest_retention_audit.py`.

Kontrakt skryptow:

- czytaja tylko lokalny `--scope-root`;
- nie wymagaja RPC;
- nie wymagaja NLN;
- nie wymagaja Spectrum;
- nie wymagaja internetu;
- nie wymagaja sekretow;
- nie importuja runtime modules;
- nie modyfikuja raw artifacts.

## 4. Entry reconstruction readiness

Command:

`python3 scripts/shadow_v2_entry_reconstruction_readiness_audit.py --scope-root reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1 --pretty`

Verdict:

`BLOCKED_ENTRY_FILLS_BLOCKED_BY_DATA`

Metrics:

| Metric | Value |
|---|---:|
| `shadow_entry_attempt_v2` rows | `127` |
| `shadow_entry_fill_v2` rows | `127` |
| entry fill `BLOCKED_BY_DATA` rows | `127` |
| entry fills with `pool_state_before` present | `0` |
| entry fills with `pool_state_after` present | `0` |
| entry fills with `fill_price` present | `0` |
| entry fills with `slippage_bps` present | `0` |
| entry fills with `own_impact_bps` present | `0` |
| entry fills with `fee_bps` present | `0` |
| entry reconstruction ready count | `0` |
| entry reconstruction blocked count | `127` |
| malformed canonical rows | `0` |

Typed blocked reasons:

| Reason | Count |
|---|---:|
| `ENTRY_FILL_BLOCKED_BY_MISSING_POOL_STATE` | `127` |
| `ENTRY_FILL_NOT_EXECUTABLE_WITHOUT_POOL_STATE_PROVENANCE` | `127` |
| `ENTRY_FILL_POOL_STATE_SAMPLE_MISSING` | `127` |

Interpretacja:

Entry attempts istnieja i sa skorelowane z canonical positions, ale entry fills nie sa reconstructable jako executable fills. Brakuje pool-state before/after, fill price, slippage, own impact i fees.

## 5. Exit reconstruction readiness

Command:

`python3 scripts/shadow_v2_exit_reconstruction_readiness_audit.py --scope-root reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1 --pretty`

Verdict:

`BLOCKED_EXIT_FILLS_BLOCKED_BY_DATA`

Metrics:

| Metric | Value |
|---|---:|
| `shadow_exit_attempt_v2` rows | `127` |
| `shadow_exit_fill_v2` rows | `127` |
| exit fill `BLOCKED_BY_DATA` rows | `127` |
| exit fills with `pool_state_before` present | `0` |
| exit fills with `pool_state_after` present | `0` |
| exit fills with `fill_price` present | `0` |
| exit fills with `slippage_bps` present | `0` |
| exit fills with `own_impact_bps` present | `0` |
| exit fills with `fee_bps` present | `0` |
| exit reconstruction ready count | `0` |
| exit reconstruction blocked count | `127` |
| terminal truth rows | `129` |
| terminal truth with `final_pnl_mark_bps` | `128` |
| terminal truth with `final_pnl_executable_bps` | `0` |
| malformed canonical rows | `0` |

Typed blocked reasons:

| Reason | Count |
|---|---:|
| `EXIT_FILL_BLOCKED_BY_MISSING_POOL_STATE` | `127` |
| `EXIT_FILL_NOT_EXECUTABLE_WITHOUT_POOL_STATE_PROVENANCE` | `127` |
| `EXIT_FILL_POOL_STATE_SAMPLE_MISSING` | `127` |
| `SESSION_ID_MISSING_FROM_LIFECYCLE_EXPLICIT_UNKNOWN` | `127` |
| `EXIT_FILL_LEGACY_EXIT_PRICE_MISSING` | `2` |
| `EXIT_FILL_LEGACY_LIFECYCLE_EXIT_BLOCKED` | `1` |

Interpretacja:

Exit attempts, exit fills i terminal truth rows istnieja, ale exit fills sa mark/legacy-derived i zablokowane dla executable reconstruction. `final_pnl_mark_bps` jest dostepny dla 128 terminal truth rows, ale `final_pnl_executable_bps` nie jest dostepny.

## 6. Replay/lifecycle terminal reconciliation

Command:

`python3 scripts/shadow_v2_replay_lifecycle_terminal_reconciliation_audit.py --scope-root reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1 --pretty`

Verdict:

`PASS_REPLAY_LIFECYCLE_RECONCILED`

Metrics:

| Metric | Value |
|---|---:|
| `shadow_replay_v2` rows | `1022` |
| `shadow_lifecycle_v2` rows | `1022` |
| replay rows derived from canonical terminal | `129` |
| lifecycle rows derived from canonical terminal | `129` |
| replay rows open_or_blocked | `893` |
| lifecycle rows open_or_blocked | `893` |
| exact join match count | `129` |
| terminal event id match count | `129` |
| terminal reason match count | `129` |
| final pnl mark match count | `129` |
| final pnl executable match count | `129` |
| close age match count | `129` |
| mismatch count | `0` |
| missing terminal link count | `0` |
| malformed replay rows | `0` |
| malformed lifecycle rows | `0` |

Interpretacja:

W granicach canonical V2 derived snapshots replay i lifecycle sa zgodne dla terminal rows. To domyka wczesniejszy problem V1, gdzie replay i lifecycle byly konkurencyjnymi prawdami. W PR18C sa derived views keyed by canonical terminal high-watermark.

Uwaga: `final_pnl_executable_match_count=129` oznacza zgodnosc wartosci pola pomiedzy replay i lifecycle. W tym scope sa to zgodne puste wartosci, bo `terminal_truth_with_final_pnl_executable_bps=0`. To nie jest dowod executable PnL.

## 7. Path density horizon evaluability

Command:

`python3 scripts/shadow_v2_path_density_horizon_audit.py --scope-root reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1 --pretty`

Verdict:

`BLOCKED_DENSITY_NOT_EVALUABLE_FOR_REQUIRED_HORIZONS`

Global metrics:

| Metric | Value |
|---|---:|
| density rows | `7154` |
| horizon count | `7` |
| malformed density rows | `0` |
| unknown horizon rows | `0` |

Per horizon:

| horizon_ms | total rows | EVALUABLE_EXACT | EVALUABLE_APPROX | SPARSE_APPROX_ONLY | NOT_EVALUABLE_NO_COVERAGE | NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY | path_points median | coverage_points median |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `2000` | `1022` | `0` | `0` | `0` | `391` | `631` | `1.0` | `1.0` |
| `3000` | `1022` | `0` | `0` | `0` | `391` | `631` | `1.0` | `1.0` |
| `10000` | `1022` | `0` | `0` | `0` | `391` | `631` | `1.0` | `1.0` |
| `30000` | `1022` | `0` | `0` | `0` | `391` | `631` | `1.0` | `1.0` |
| `120000` | `1022` | `0` | `0` | `0` | `384` | `638` | `1.0` | `1.0` |
| `300000` | `1022` | `0` | `0` | `0` | `384` | `638` | `1.0` | `1.0` |
| `500000` | `1022` | `0` | `0` | `0` | `384` | `638` | `1.0` | `1.0` |

Interpretacja:

Density rows sa poprawnie emitowane, ale w tym scope nie ma horyzontu, ktory przechodzi jako evaluable. To blokuje wnioski o 2s/3s, 120s, 300s i 500s na podstawie tego runu.

## 8. Temporal/no-lookahead evidence audit

Command:

`python3 scripts/shadow_v2_temporal_no_lookahead_audit.py --scope-root reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1 --pretty`

Verdict:

`BLOCKED_TEMPORAL_AMBIGUITY_REMAINS`

Temporal classes per event family:

| Event family | Temporal classes |
|---|---|
| `shadow_position_v2` | `POST_ENTRY=129`, `UNKNOWN=1` |
| `shadow_entry_attempt_v2` | `POST_ENTRY=127` |
| `shadow_entry_fill_v2` | `POST_ENTRY=127` |
| `shadow_path_sample_v2` | `POST_ENTRY=255` |
| `shadow_exit_attempt_v2` | `POST_ENTRY=127` |
| `shadow_exit_fill_v2` | `POST_EXIT=127` |
| `shadow_terminal_truth_v2` | `POST_EXIT=129` |

Clock domains per event family:

| Event family | Clock domains |
|---|---|
| `shadow_position_v2` | `WALL_CLOCK_MS=130` |
| `shadow_entry_attempt_v2` | `SUBMIT_TS_MS=127` |
| `shadow_entry_fill_v2` | `LANDING_TS_MS=127` |
| `shadow_path_sample_v2` | `STREAM_OBSERVED_MS=255` |
| `shadow_exit_attempt_v2` | `STREAM_OBSERVED_MS=127` |
| `shadow_exit_fill_v2` | `LANDING_TS_MS=127` |
| `shadow_terminal_truth_v2` | `STREAM_OBSERVED_MS=129` |

Ordering metrics:

| Metric | Value |
|---|---:|
| event_order_key present rows | `763` |
| event_order_key missing rows | `259` |
| non-monotonic `event_seq_in_process` | `0` |
| post-entry fields used in pre-decision context | `0` |
| terminal truth used as pre-entry evidence | `0` |
| derived replay/lifecycle used as canonical input | `0` |
| malformed canonical rows | `0` |
| malformed replay rows | `0` |
| malformed lifecycle rows | `0` |

Explicit UNKNOWN chain-order components:

| Component | Rows |
|---|---:|
| `block_time` | `763` |
| `transaction_index_or_unknown` | `763` |
| `instruction_index_or_unknown` | `763` |
| `inner_instruction_index_or_unknown` | `763` |
| `log_index_or_unknown` | `763` |
| `signature` | `509` |

Interpretacja:

Nie wykryto bezposredniej lookahead/order violation. Blokada wynika z jawnej temporal ambiguity: brak pelnego chain order dla wielu zdarzen i brak event order key dla terminal/smoke/position-family rows. To jest audit-safe, ale nie jest enough do mocnych ordering-sensitive conclusions.

## 9. Manifest/retention audit

Command:

`python3 scripts/shadow_v2_manifest_retention_audit.py --scope-root reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1 --pretty`

Verdict:

`PASS_MANIFEST_RETENTION_AUDIT`

Metrics:

| Metric | Value |
|---|---|
| runtime `post_run_manifest.status` | `PASS` |
| strict audit status | `PASS` |
| manifest blockers | `[]` |
| artifact count | `6` |
| total size bytes | `25059202` |
| raw JSONL not staged | `true` |
| logs not staged | `true` |
| runtime scope not staged | `true` |
| local configs not staged | `true` |
| forbidden staged files | `[]` |

Schema coverage counts:

| Schema | Rows |
|---|---:|
| `shadow_position_event_v2` | `1022` |
| `shadow_replay_v2` | `1022` |
| `shadow_lifecycle_v2` | `1022` |
| `shadow_path_density_v2` | `7154` |

Post-run strict manifest audit command:

`python3 scripts/shadow_v2_manifest_audit.py --scope-root reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1 --manifest-phase post_run --schema-manifest reports/selector/shadow_v2_required_schema_manifest.csv --acceptance-gates reports/selector/shadow_v2_acceptance_gates.csv --strict`

Result:

- status: `PASS`;
- blockers: `[]`;
- artifact_count: `7`;
- total_size_bytes: `25063619`.

## 10. Approval flags

Nie ustawiono i nie przyznano:

| Flag | Value |
|---|---|
| `research_grade` | `NOT_GRANTED` |
| `live_equivalence` | `NOT_GRANTED` |
| `runtime_approval` | `false` |
| `shadow_close_only_approval` | `false` |
| `active_close_approval` | `false` |
| `strategy_research_unblocked` | `false` |

Kandydatury:

| Candidate | Value |
|---|---|
| `runtime_approval_candidate` | `true_for_shadow_v2_logging_validation_only` |
| `strategy_research_unblocked_candidate` | `false_until_executable_fill_provenance_and_density_temporal_blocks_are_resolved` |

## 11. Co jest udowodnione

Udowodnione:

- offline scripts moga czytac PR18C scope deterministycznie bez runtime/RPC;
- manifest i retention gates przechodza;
- canonical wrapper schema jest parsowalny, malformed rows = `0`;
- replay i lifecycle derived terminal snapshots sa zgodne dla `129` terminal rows;
- terminal event id, terminal reason, final mark PnL, final executable PnL field values i close age matchuja pomiedzy replay/lifecycle;
- raw JSONL/log/runtime scope/local configs nie musza byc commitowane do raportu.

## 12. Co jest zablokowane

Zablokowane:

- entry executable reconstruction: `entry_reconstruction_ready_count=0`;
- exit executable reconstruction: `exit_reconstruction_ready_count=0`;
- executable PnL: `terminal_truth_with_final_pnl_executable_bps=0`;
- density evaluability: no `EVALUABLE_*` or `SPARSE_APPROX_ONLY` rows for required horizons;
- strong temporal ordering claims: explicit UNKNOWN chain-order components remain.

## 13. Final decision

Final verdict:

`BLOCKED_EXECUTABLE_FILL_PROVENANCE_MISSING`

Uzasadnienie:

- schema/join nie jest popsuty;
- replay/lifecycle reconciliation przechodzi;
- manifest/retention przechodzi;
- nie wykryto lookahead violation;
- ale entry i exit fill evidence pozostaja `BLOCKED_BY_DATA` przez brak pool-state provenance i executable fill fields.

Konsekwencja:

- PR18C evidence moze byc uzyte do dalszego debugowania Shadow V2 logging/reconciliation;
- nie moze byc uzyte jako proof live-equivalent fill/PnL;
- nie moze odblokowac research-grade conclusions;
- nie moze odblokowac `shadow_close_only`;
- nie moze odblokowac active close;
- nie moze odblokowac runtime approval.
