# ADR-8D: Shadow V2 PR18E Validation Burnin

Data: 2026-07-02

Status:

```text
ACCEPTED_AS_BLOCKED_EVIDENCE
```

## D1. Problem

Po PR30 trzeba bylo sprawdzic, czy canonical pool-state + fill provenance realnie poprawia offline reconstruction readiness Shadow V2.

Poprzedni stan PR18D byl zablokowany:

```text
final_verdict = BLOCKED_EXECUTABLE_FILL_PROVENANCE_MISSING
entry_reconstruction_ready_count = 0
exit_reconstruction_ready_count = 0
entry_fill_BLOCKED_BY_DATA = 127
exit_fill_BLOCKED_BY_DATA = 127
terminal_truth_with_final_pnl_executable_bps = 0
density_evaluable_rows = 0
temporal ambiguity remains
```

## D2. Decyzja

Wykonano jeden kontrolowany validation/fidelity-only burnin:

```text
run_id = shadow-burnin-v2-fidelity-validation-pr18e-r1
scope = reports/selector/shadow-v2-fidelity-validation-pr18e-r1
main_head = f6f055f16a8e546ec57a269a4b6b86586c0c01ad
PR30 merge commit = f6f055f16a8e546ec57a269a4b6b86586c0c01ad
```

Decyzja po audytach:

```text
BLOCKED_DENSITY_OR_TEMPORAL_AUDIT
```

Runtime writer/materializer/shutdown sa wystarczajaco potwierdzone dla logging validation, ale offline reconstruction readiness pozostaje zablokowane.

## D3. Evidence

Runtime:

- `pre_run_manifest.status = PASS`
- pre-run strict audit = PASS
- validation burnin plan audit = PASS
- legacy downgrade audit = PASS
- launcher preflight = PASS
- `post_run_manifest.status = PASS`
- post-run strict audit = PASS
- `PostBuyRuntime` generated and strict-verified post-run manifest before shutdown
- `All components shut down successfully`
- `Ghost Launcher shutdown complete`
- SIGTERM = false
- forced abort = false
- `Transport channel disconnected` after shutdown = 0

Canonical evidence:

| schema | rows |
|---|---:|
| `shadow_position_v2` | 190 |
| `pool_state_sample_v2` | 185 |
| `shadow_entry_attempt_v2` | 176 |
| `shadow_entry_fill_v2` | 176 |
| `shadow_path_sample_v2` | 373 |
| `shadow_exit_attempt_v2` | 185 |
| `shadow_exit_fill_v2` | 185 |
| `shadow_terminal_truth_v2` | 189 |

Po odjeciu markera diagnostycznego:

```text
real_shadow_v2_positions = 189
malformed_canonical_rows = 0
```

Offline audits:

| Audit | Verdict |
|---|---|
| entry reconstruction readiness | `BLOCKED_ENTRY_FILLS_BLOCKED_BY_DATA` |
| exit reconstruction readiness | `BLOCKED_EXIT_FILLS_BLOCKED_BY_DATA` |
| replay/lifecycle reconciliation | `PASS_REPLAY_LIFECYCLE_RECONCILED` |
| path density horizon evaluability | `BLOCKED_DENSITY_NOT_EVALUABLE_FOR_REQUIRED_HORIZONS` |
| temporal/no-lookahead | `FAIL_LOOKAHEAD_OR_ORDERING_VIOLATION` |
| manifest/retention | `PASS_MANIFEST_RETENTION_AUDIT` |

## D4. Root cause / current blockers

Entry fill:

- 176/176 `shadow_entry_fill_v2` rows sa `BLOCKED_BY_DATA`.
- 0/176 ma `pool_state_before`.
- 0/176 ma `pool_state_after`.
- 0/176 ma `fill_price`, `slippage_bps`, `own_impact_bps`, `fee_bps`.
- Row-level reasons obejmuja `ENTRY_POOL_STATE_BEFORE_UNAVAILABLE`, `ENTRY_POOL_STATE_AFTER_UNAVAILABLE`, `FILL_PRICE_UNAVAILABLE`, `SLIPPAGE_BPS_UNAVAILABLE`, `OWN_IMPACT_BPS_UNAVAILABLE`, `FEE_BPS_UNAVAILABLE`.

Exit fill:

- 185/185 `shadow_exit_fill_v2` rows sa `BLOCKED_BY_DATA`.
- 185/185 ma `pool_state_before`, co jest poprawa po PR30.
- 0/185 ma `pool_state_after`.
- 0/185 ma `fill_price`, `slippage_bps`, `own_impact_bps`, `fee_bps`.
- `POOL_STATE_ACCOUNT_DATA_HASH_MISSING = 185`.

Terminal:

- 189 `shadow_terminal_truth_v2` rows.
- 188 z `final_pnl_mark_bps`.
- 0 z `final_pnl_executable_bps`.

Density:

- `EVALUABLE_EXACT = 0`
- `EVALUABLE_APPROX = 0`
- `SPARSE_APPROX_ONLY = 0`
- `NOT_EVALUABLE_NO_COVERAGE = 3816`
- `NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY = 7797`

Temporal:

- `event_order_key_missing_rows = 379`
- `non_monotonic_event_seq_in_process = 1`
- `post_entry_fields_used_in_pre_decision_context = 0`
- `terminal_truth_used_as_pre_entry_evidence = 0`
- `derived_replay_lifecycle_used_as_canonical_input = 0`

## D5. Granice runtime

Ten burnin i raport nie zmieniaja runtime behavior.

Nadal obowiazuje:

```text
runtime_approval = false
shadow_close_only_approval = false
active_close_approval = false
research_grade = NOT_GRANTED
live_equivalence = NOT_GRANTED
strategy_research_unblocked = false
```

Nie zmieniono:

- BUY/REJECT
- Gatekeeper policy
- selector runtime
- TX/Jito/live path
- `shadow_close_only`
- active close
- R51

## D6. Konsekwencje

1. Shadow V2 logging validation harness ma potwierdzona zdolnosc do clean shutdown, manifest flush i durable canonical/derived artifact production na scope PR18E.
2. Replay/lifecycle reconciliation dla V2 canonical stream jest potwierdzony w tym runie.
3. PR30 poprawil exit `pool_state_before` coverage, ale nie wystarcza do executable sell fill reconstruction.
4. Entry fill provenance pozostaje zablokowane.
5. Long/short horizon density pozostaje nieewaluowalne.
6. Temporal ordering nadal ma twarda blokade przez missing event order i non-monotonic sequence.
7. Nie wolno promowac tego wyniku do runtime approval, active close, RCE proof ani live-equivalence.

## D7. Rejected alternatives

Odrzucono:

- traktowanie `pool_state_before` po stronie exit jako wystarczajacego dowodu executable fill;
- traktowanie mark PnL jako executable PnL;
- ignorowanie `event_order_key_missing_rows`;
- uznanie density horizons za approx przy zerowym `EVALUABLE_*`;
- commitowanie raw JSONL/runtime scope do PR.

## D8. Follow-up

Wymagane przed kolejnym readiness claim:

1. Entry pool-state source dla entry fill albo explicit permanent downgrade do mark-only entry.
2. Exit pool-state after, account hash, fill price, fee/slippage/own-impact albo explicit executable-blocked contract.
3. Path sampling fix lub validation mode, ktory produkuje evaluable density rows.
4. Event-order fix: brak missing `event_order_key` dla canonical rows wymaganych przez temporal audit, brak non-monotonic `event_seq_in_process`.
5. Ponowny validation/fidelity burnin i offline audits.

Dozwolona kandydatura po PR18E:

```text
runtime_approval_candidate = true_for_shadow_v2_logging_validation_only
```

Niedozwolone:

```text
research_grade = true
live_equivalence = true
runtime_approval = true
shadow_close_only_approval = true
active_close_approval = true
strategy_research_unblocked = true
```
