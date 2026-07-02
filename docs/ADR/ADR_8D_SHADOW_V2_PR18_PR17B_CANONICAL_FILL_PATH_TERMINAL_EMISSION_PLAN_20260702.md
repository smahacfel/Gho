# ADR-8D: Shadow V2 PR18 / PR17B Canonical Fill Path Terminal Emission Plan

## Status

Proposed plan, not implementation.

## D1. Problem

PR17-r2 wygenerowal realna pozycje Shadow V2, ale canonical V2 evidence nadal zatrzymuje sie na `POSITION_CREATED`.

Entry, exit i close istnieja w legacy shadow artifacts:

- `shadow_entries.jsonl`;
- `buys.jsonl`;
- `shadow_lifecycle.jsonl`.

Nie istnieja jeszcze jako canonical V2:

- `shadow_entry_attempt_v2`;
- `shadow_entry_fill_v2`;
- `shadow_path_sample_v2`;
- `shadow_exit_attempt_v2`;
- `shadow_exit_fill_v2`;
- `shadow_terminal_truth_v2`.

Bez tych records nie da sie zrobic V2-only fidelity reconstruction ani terminal reconciliation.

## D2. Decyzja

Przygotowujemy PR18 / PR17B jako waski etap implementacyjny:

`CANONICAL_V2_FILL_PATH_TERMINAL_EMISSION`

Implementacja ma pozostac side-by-side i logging-only. Wszystkie nowe records ida przez `ShadowV2ValidationHarness::append_record(...)`.

Nie zmieniamy decyzji runtime ani execution path.

## D3. Kontekst

Istniejace typy sa juz w:

`ghost-brain/src/guardian/post_buy/shadow_v2.rs`

Istniejacy runtime adapter jest w:

`ghost-launcher/src/components/post_buy_runtime.rs`

Obecnie runtime emituje:

- smoke marker;
- `ShadowPositionV2` dla accepted shadow handoff.

Plan zaklada wykorzystanie juz istniejacych helperow:

- `ShadowEntryFillV2::from_static_buy_model`;
- `ShadowPathSampleV2::from_pool_state_mark`;
- `ShadowExitAttemptV2::from_mark_path_trigger`;
- `ShadowExitFillV2::from_static_sell_model`;
- `ShadowTerminalTruthV2`;
- derived replay/lifecycle builders.

## D4. Dowody

PR17-r2 report:

- `real_shadow_v2_positions=1`;
- `shadow_position_event_v2 rows=2`;
- `shadow_replay_v2 rows=2`;
- `shadow_lifecycle_v2 rows=2`;
- `shadow_path_density_v2 rows=14`;
- all density rows `NOT_EVALUABLE_NO_COVERAGE`;
- legacy `shadow_entries.jsonl rows=1`;
- legacy `buys.jsonl rows=1`;
- legacy `shadow_lifecycle.jsonl rows=3`;
- close reason `TimeStop`;
- final_pnl_pct `-17.154999999999998`.

To dowodzi, ze runtime ma dane do realnego handoff, ale nie dowodzi jeszcze V2-only fidelity.

## D5. Invariants

PR18 / PR17B musi zachowac:

- no BUY/REJECT change;
- no Gatekeeper policy change;
- no selector runtime change;
- no TX/Jito/live path change;
- no `shadow_close_only`;
- no active close;
- no runtime approval;
- no live-equivalence claim;
- no R51;
- no raw JSONL/log/runtime artifacts staged.

Shadow V2 records nie moga byc konsumowane przez decyzje.

## D6. Consequences

Po implementacji nastepny validation burnin powinien moc udowodnic:

- V2-only entry attempt/fill evidence;
- V2-only path samples;
- V2-only exit attempt/fill evidence;
- V2 terminal truth;
- density evaluable where coverage exists;
- replay/lifecycle terminal reconciliation.

Jesli dane runtime sa niepelne, PR18 ma emitowac typed missing reasons zamiast falszywego sukcesu.

## D7. Rejected Alternatives

Odrzucone:

- traktowanie legacy `shadow_lifecycle.jsonl` jako canonical V2 terminal truth;
- przepisywanie raw JSONL offline do V2 bez runtime source refs;
- zmiana Gatekeeper progow albo BUY/REJECT w celu generowania wiecej probek;
- wlaczenie live-equivalence claim bez PR14 calibration;
- podlaczenie Shadow V2 do policy/selector.

## D8. Required Follow-Up

Po implementacji PR18 / PR17B:

1. uruchomic osobny V2-only fidelity validation burnin;
2. wymagac `path_samples > 0`;
3. wymagac entry/exit fill rows albo typed blocked rows;
4. wymagac terminal truth;
5. wymagac replay/lifecycle terminal reconciliation;
6. przygotowac report-only PR z wynikiem.

Do tego czasu:

- `research_grade=NOT_GRANTED`;
- `live_equivalence=NOT_GRANTED`;
- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`.
