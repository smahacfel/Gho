# Raport Shadow V2 PR18E Validation Burnin 2026-07-02

## 1. Executive verdict

Finalny verdict:

```text
BLOCKED_DENSITY_OR_TEMPORAL_AUDIT
```

Runtime validation burnin po PR30 przeszedl bramki uruchomieniowe, manifestowe i shutdownowe, ale nie odblokowuje jeszcze research-grade ani executable-fill research.

Najwazniejsze rozstrzygniecia:

- `runtime post_run_manifest.status = PASS`
- `post-run strict audit = PASS`
- `clean_shutdown_proven = true`
- `real_shadow_v2_positions = 189`
- `malformed canonical rows = 0`
- `PASS_REPLAY_LIFECYCLE_RECONCILED`
- `PASS_MANIFEST_RETENTION_AUDIT`
- `entry_reconstruction_ready_count = 0`
- `exit_reconstruction_ready_count = 0`
- `terminal_truth_with_final_pnl_executable_bps = 0`
- `density_evaluable_rows = 0`
- `temporal audit = FAIL_LOOKAHEAD_OR_ORDERING_VIOLATION`

PR30 poprawil strone exit provenance: `shadow_exit_fill_v2.pool_state_before` jest obecny dla 185/185 exit fill rows. Nie poprawil jednak entry provenance i nie daje executable fill reconstruction, bo fill price, slippage, own impact, fee i pool state after nadal sa niedostepne. Density i temporal ordering nadal blokuja wniosek research-grade.

## 2. Zakres i granice

Run:

```text
run_id = shadow-burnin-v2-fidelity-validation-pr18e-r1
scope = reports/selector/shadow-v2-fidelity-validation-pr18e-r1
main_head = f6f055f16a8e546ec57a269a4b6b86586c0c01ad
PR30 merge commit = f6f055f16a8e546ec57a269a4b6b86586c0c01ad
```

Zakres byl wylacznie validation/fidelity-only. Ten raport nie jest strategy proof, edge proof, runtime approval, live-equivalence claim ani active-close proof.

Zachowane flagi:

```text
research_grade = NOT_GRANTED
live_equivalence = NOT_GRANTED
runtime_approval = false
shadow_close_only_approval = false
active_close_approval = false
strategy_research_unblocked = false
```

Nie zmieniano BUY/REJECT, Gatekeeper policy, selector runtime, TX/Jito/live path, `shadow_close_only`, active close ani R51.

## 3. Pre-run i runtime

Pre-run gates:

| Gate | Wynik |
|---|---:|
| `pre_run_manifest.status` | PASS |
| pre-run strict manifest audit | PASS |
| validation burnin plan audit | PASS |
| legacy downgrade audit | PASS |
| launcher preflight | PASS |

Runtime controller:

| Pole | Wartosc |
|---|---:|
| start UTC | `2026-07-02T13:34:02Z` |
| SIGINT UTC | `2026-07-02T14:24:08Z` |
| end UTC | `2026-07-02T14:24:40Z` |
| elapsed do SIGINT | 3000 s |
| elapsed total | 3038 s |
| controller exit status | 0 |
| shutdown method | SIGINT |
| SIGTERM | false |
| forced component abort | false |
| `Transport channel disconnected` po shutdownie | 0 |

Shutdown evidence:

- `PostBuyRuntime: Shadow V2 post-run manifest generated and strict-verified`
- `PostBuyRuntime shut down successfully`
- `Seer shut down successfully`
- `Trigger shut down successfully`
- `SnapshotListener shut down successfully`
- `GatekeeperCommitLoop shut down successfully`
- `LivePipelineFlushLoop shut down successfully`
- `Watchdog shut down successfully`
- `All components shut down successfully`
- `Ghost Launcher shutdown complete`

## 4. Manifest i retention

Post-run manifest:

| Pole | Wartosc |
|---|---:|
| `post_run_manifest.status` | PASS |
| `post_run_manifest.blockers` | `[]` |
| post-run strict audit | PASS |
| manifest/retention audit | `PASS_MANIFEST_RETENTION_AUDIT` |
| artifact count | 5 |
| total size bytes | 41848518 |
| raw JSONL staged | false |
| logs staged | false |
| runtime scope staged | false |
| local configs staged | false |

Schema coverage z manifest retention:

| Artifact | Rows |
|---|---:|
| `shadow_position_event_v2` | 1659 |
| `shadow_replay_v2` | 1659 |
| `shadow_lifecycle_v2` | 1659 |
| `shadow_path_density_v2` | 11613 |

## 5. Canonical V2 evidence

Po odjeciu markera diagnostycznego run zawiera realne Shadow V2 positions:

| Metryka | Wartosc |
|---|---:|
| all positions | 190 |
| validation smoke marker positions | 1 |
| real_shadow_v2_positions | 189 |
| malformed canonical rows | 0 |

Event kind counts:

| event_kind | Rows |
|---|---:|
| `POSITION_CREATED` | 190 |
| `POOL_STATE_SAMPLE` | 185 |
| `ENTRY_ATTEMPT` | 176 |
| `ENTRY_FILL` | 176 |
| `PATH_SAMPLE` | 373 |
| `EXIT_ATTEMPT` | 185 |
| `EXIT_FILL` | 185 |
| `TERMINAL_TRUTH` | 189 |

Schema counts:

| schema | Rows |
|---|---:|
| `shadow_position_v2` | 190 |
| `pool_state_sample_v2` | 185 |
| `shadow_entry_attempt_v2` | 176 |
| `shadow_entry_fill_v2` | 176 |
| `shadow_path_sample_v2` | 373 |
| `shadow_exit_attempt_v2` | 185 |
| `shadow_exit_fill_v2` | 185 |
| `shadow_terminal_truth_v2` | 189 |

## 6. Entry provenance

Verdict entry audit:

```text
BLOCKED_ENTRY_FILLS_BLOCKED_BY_DATA
```

Entry metrics:

| Metryka | Wartosc |
|---|---:|
| `shadow_entry_attempt_v2` rows | 176 |
| `shadow_entry_fill_v2` rows | 176 |
| `entry_fill BLOCKED_BY_DATA` rows | 176 |
| with `pool_state_before` | 0 |
| with `pool_state_after` | 0 |
| with `fill_price` | 0 |
| with `slippage_bps` | 0 |
| with `own_impact_bps` | 0 |
| with `fee_bps` | 0 |
| entry reconstruction ready | 0 |
| entry reconstruction blocked | 176 |

Row-level typed blocked reasons:

| Reason | Rows |
|---|---:|
| `ENTRY_POOL_STATE_BEFORE_UNAVAILABLE` | 176 |
| `ENTRY_POOL_STATE_AFTER_UNAVAILABLE` | 176 |
| `ENTRY_FILL_POOL_STATE_SAMPLE_MISSING` | 176 |
| `ENTRY_FILL_POOL_STATE_SAMPLE_NOT_AVAILABLE_IN_RUNTIME_HANDOFF` | 176 |
| `ENTRY_FILL_NOT_EXECUTABLE_WITHOUT_POOL_STATE_PROVENANCE` | 176 |
| `FILL_PRICE_UNAVAILABLE` | 176 |
| `SLIPPAGE_BPS_UNAVAILABLE` | 176 |
| `OWN_IMPACT_BPS_UNAVAILABLE` | 176 |
| `FEE_BPS_UNAVAILABLE` | 176 |
| `LANDING_TELEMETRY_UNAVAILABLE` | 176 |
| `QUOTE_FILL_DIVERGENCE_UNAVAILABLE` | 176 |

Wniosek: entry executable fill nadal nie jest rekonstruowalny. Entry price/fill nie moze byc traktowany jako executable live fill.

## 7. Exit provenance

Verdict exit audit:

```text
BLOCKED_EXIT_FILLS_BLOCKED_BY_DATA
```

Exit metrics:

| Metryka | Wartosc |
|---|---:|
| `shadow_exit_attempt_v2` rows | 185 |
| `shadow_exit_fill_v2` rows | 185 |
| `exit_fill BLOCKED_BY_DATA` rows | 185 |
| with `pool_state_before` | 185 |
| with `pool_state_after` | 0 |
| with `fill_price` | 0 |
| with `slippage_bps` | 0 |
| with `own_impact_bps` | 0 |
| with `fee_bps` | 0 |
| exit reconstruction ready | 0 |
| exit reconstruction blocked | 185 |

Row-level typed blocked reasons:

| Reason | Rows |
|---|---:|
| `EXIT_POOL_STATE_AFTER_UNAVAILABLE` | 185 |
| `POOL_STATE_ACCOUNT_DATA_HASH_MISSING` | 185 |
| `SESSION_ID_MISSING_FROM_LIFECYCLE_EXPLICIT_UNKNOWN` | 185 |
| `FILL_PRICE_UNAVAILABLE` | 185 |
| `SLIPPAGE_BPS_UNAVAILABLE` | 185 |
| `OWN_IMPACT_BPS_UNAVAILABLE` | 185 |
| `FEE_BPS_UNAVAILABLE` | 185 |
| `LANDING_TELEMETRY_UNAVAILABLE` | 185 |
| `QUOTE_FILL_DIVERGENCE_UNAVAILABLE` | 185 |
| `EXIT_FILL_LEGACY_EXIT_PRICE_MISSING` | 2 |
| `EXIT_FILL_LEGACY_LIFECYCLE_EXIT_BLOCKED` | 1 |

Wniosek: PR30 poprawil exit pool-state-before link coverage do 185/185, ale executable exit fill nadal jest zablokowany. Exit result pozostaje path/legacy-lifecycle evidence, nie executable sell fill.

## 8. Terminal truth

Terminal metrics:

| Metryka | Wartosc |
|---|---:|
| `shadow_terminal_truth_v2` rows | 189 |
| with `final_pnl_mark_bps` | 188 |
| with `final_pnl_executable_bps` | 0 |

`final_pnl_executable_bps` poprawnie pozostaje null, poniewaz `shadow_exit_fill_v2` nadal jest `BLOCKED_BY_DATA`.

## 9. Replay/lifecycle reconciliation

Verdict:

```text
PASS_REPLAY_LIFECYCLE_RECONCILED
```

Metrics:

| Metryka | Wartosc |
|---|---:|
| `shadow_replay_v2` rows | 1659 |
| `shadow_lifecycle_v2` rows | 1659 |
| rows derived from canonical terminal | 189 / 189 |
| rows open or blocked | 1470 / 1470 |
| exact join match count | 189 |
| terminal event id match count | 189 |
| terminal reason match count | 189 |
| final pnl mark match count | 189 |
| final pnl executable match count | 189 |
| close age match count | 189 |
| mismatch count | 0 |
| missing terminal link count | 0 |

Wniosek: PR18E nie zlamal kontraktu, ze derived replay/lifecycle pochodza z canonical stream. To nie wystarcza do research-grade, ale usuwa stary V1 problem replay/lifecycle mismatch dla tej sciezki.

## 10. Path density

Verdict:

```text
BLOCKED_DENSITY_NOT_EVALUABLE_FOR_REQUIRED_HORIZONS
```

Aggregate density:

| Verdict | Rows |
|---|---:|
| `EVALUABLE_EXACT` | 0 |
| `EVALUABLE_APPROX` | 0 |
| `SPARSE_APPROX_ONLY` | 0 |
| `NOT_EVALUABLE_NO_COVERAGE` | 3816 |
| `NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY` | 7797 |

Per horizon:

| horizon_ms | total | EVALUABLE_EXACT | EVALUABLE_APPROX | SPARSE_APPROX_ONLY | NOT_EVALUABLE_NO_COVERAGE | NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY |
|---:|---:|---:|---:|---:|---:|---:|
| 2000 | 1659 | 0 | 0 | 0 | 549 | 1110 |
| 3000 | 1659 | 0 | 0 | 0 | 549 | 1110 |
| 10000 | 1659 | 0 | 0 | 0 | 549 | 1110 |
| 30000 | 1659 | 0 | 0 | 0 | 549 | 1110 |
| 120000 | 1659 | 0 | 0 | 0 | 540 | 1119 |
| 300000 | 1659 | 0 | 0 | 0 | 540 | 1119 |
| 500000 | 1659 | 0 | 0 | 0 | 540 | 1119 |

Wniosek: zadne wymagane horizon claims nie sa ewaluowalne z tego runa. Nie wolno z niego wyciagac wnioskow dla 2s/3s ani 300s/500s.

## 11. Temporal/no-lookahead

Verdict:

```text
FAIL_LOOKAHEAD_OR_ORDERING_VIOLATION
```

Temporal metrics:

| Metryka | Wartosc |
|---|---:|
| malformed canonical rows | 0 |
| event_order_key present rows | 1280 |
| event_order_key missing rows | 379 |
| non_monotonic_event_seq_in_process | 1 |
| post_entry_fields_used_in_pre_decision_context | 0 |
| terminal_truth_used_as_pre_entry_evidence | 0 |
| derived_replay_lifecycle_used_as_canonical_input | 0 |

Explicit UNKNOWN chain-order components:

| Component | Count |
|---|---:|
| `block_time` | 1280 |
| `signature` | 928 |
| `transaction_index_or_unknown` | 1280 |
| `instruction_index_or_unknown` | 1280 |
| `inner_instruction_index_or_unknown` | 1280 |
| `log_index_or_unknown` | 1280 |

Wniosek: nie ma dowodu, ze terminal truth albo post-entry pola sa uzywane jako pre-entry evidence, ale event ordering nadal nie spelnia kontraktu badawczego. Blokada wynika z brakujacych `event_order_key` rows i jednego non-monotonic `event_seq_in_process`.

## 12. Finalna decyzja

Runtime harness:

```text
PASS dla manifest/shutdown/writer/materializer na scope PR18E
```

Offline research readiness:

```text
NOT_GRANTED
```

Final verdict:

```text
BLOCKED_DENSITY_OR_TEMPORAL_AUDIT
```

Drugorzedne blokery:

- `BLOCKED_ENTRY_POOL_STATE_STILL_UNAVAILABLE`
- `BLOCKED_EXIT_FILLS_BLOCKED_BY_DATA`
- `BLOCKED_ENTRY_FILLS_BLOCKED_BY_DATA`
- `BLOCKED_DENSITY_NOT_EVALUABLE_FOR_REQUIRED_HORIZONS`
- `FAIL_LOOKAHEAD_OR_ORDERING_VIOLATION`

Mozliwe kandydatury, bez automatycznego approval:

```text
runtime_approval_candidate = true_for_shadow_v2_logging_validation_only
strategy_research_unblocked_candidate = false_until_density_and_temporal_audit_pass
```

Nie wolno uzywac tego runa jako dowodu live PnL, executable fills, live slippage behavior, landing outcome, active close ani RCE/runtime approval.

## 13. Nastepne wymagane prace

1. Entry side: doprowadzic `shadow_entry_fill_v2` do realnego linkowania `pool_state_sample_v2` albo utrzymac `BLOCKED_BY_DATA` z jeszcze dokladniejszym typed source reason.
2. Exit side: uzupelnic `pool_state_after`, `account_data_hash`, fill price, fee/slippage/own-impact albo jawnie ograniczyc contract do mark/path-only.
3. Density: zwiekszyc faktyczna liczbe path samples i/lub poprawic sampler, tak aby required horizons mialy `EVALUABLE_*` rows.
4. Temporal: usunac `event_order_key_missing_rows` i non-monotonic `event_seq_in_process`; UNKNOWN chain order moze pozostac tylko jako jawna ambiguity, nie jako exact ordering.
5. Dopiero po tym uruchomic kolejny validation/fidelity burnin i ponownie przejsc offline audits.

## 14. Evidence paths

Surowe evidence istnieje lokalnie i nie jest czescia PR:

```text
reports/selector/shadow-v2-fidelity-validation-pr18e-r1/
/tmp/pr18e_r1_entry_audit.json
/tmp/pr18e_r1_exit_audit.json
/tmp/pr18e_r1_reconciliation_audit.json
/tmp/pr18e_r1_density_audit.json
/tmp/pr18e_r1_temporal_audit.json
/tmp/pr18e_r1_manifest_retention_audit.json
/tmp/pr18e_r1_post_run_strict_audit.json
/tmp/pr18e_r1_metrics.json
```

Do PR wchodza tylko raporty pochodne, bez raw JSONL, logow, runtime scope i lokalnych configow.
