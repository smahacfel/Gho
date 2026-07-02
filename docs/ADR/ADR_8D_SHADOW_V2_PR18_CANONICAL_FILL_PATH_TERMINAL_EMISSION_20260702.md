# ADR-8D: Shadow V2 PR18 Canonical Fill Path Terminal Emission

## Status

Proposed implementation, ready for review.

## D1. Problem

PR17-r2 udowodnil, ze Shadow V2 logging-only harness moze wygenerowac realny `shadow_position_v2` dla accepted shadow handoff, ale canonical V2 evidence zatrzymywalo sie na `POSITION_CREATED`.

Entry, exit, path i terminal evidence nadal byly dostepne tylko przez legacy shadow artifacts albo nie byly emitowane jako canonical V2 records. To blokowalo V2-only fidelity validation, bo audyt nie mogl wymagac:

- `shadow_entry_attempt_v2`;
- `shadow_entry_fill_v2`;
- `shadow_path_sample_v2`;
- `shadow_exit_attempt_v2`;
- `shadow_exit_fill_v2`;
- `shadow_terminal_truth_v2`.

## D2. Decyzja

Dodajemy waski, side-by-side adapter emission dla canonical Shadow V2 fill/path/terminal evidence.

Decyzja obejmuje:

- entry attempt/fill emission z accepted shadow handoff w `PostBuyRuntime`;
- path sample, exit attempt/fill i terminal truth emission z `PostBuyGuardian` lifecycle evidence;
- wszystkie rekordy przez `ShadowV2ValidationHarness::append_record(...)`;
- typed `BLOCKED_BY_DATA` dla entry/exit fill, jezeli brakuje executable pool-state/fill evidence;
- explicit limitations i source refs zamiast cichego overclaimu.

Decyzja nie obejmuje:

- zmian BUY/REJECT;
- zmian Gatekeeper policy;
- zmian selector runtime;
- zmian TX/Jito/live path;
- `shadow_close_only`;
- active close;
- runtime approval;
- live-equivalence claim.

## D3. Kontekst

Plan PR18 / PR17B okreslil, ze nastepny validation burnin ma wymagac V2-only:

- entry fill evidence;
- exit fill evidence;
- terminal truth;
- path samples > 0;
- density evaluable where coverage exists;
- replay/lifecycle terminal reconciliation.

Bez canonical V2 emission kolejny burnin powtorzylby ograniczenie PR17-r2:

`CANONICAL_V2_FILL_PATH_TERMINAL_RECORDS_NOT_EMITTED`

## D4. Dowody implementacyjne

Pliki zmienione:

- `ghost-launcher/src/components/post_buy_runtime.rs`;
- `ghost-brain/src/guardian/post_buy/engine.rs`;
- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`.

Nowe zachowanie:

- `PostBuyRuntime` emituje `shadow_entry_attempt_v2` i `shadow_entry_fill_v2` po accepted shadow handoff;
- `MonitoringEngine` dostaje opcjonalny shared `ShadowV2ValidationHarness`;
- legacy `shadow_lifecycle` events emitujace close/path evidence generuja canonical V2 path/exit/terminal records;
- derived replay/lifecycle/density pozostaja generowane przez harness z canonical stream.

Testy wykonane:

- `cargo check -p ghost-brain -q` - PASS;
- `cargo check -p ghost-launcher -q` - PASS;
- `cargo test -p ghost-brain shadow_v2_lifecycle_close_emits_path_exit_terminal_records -- --nocapture` - PASS;
- `cargo test -p ghost-launcher shadow_v2_entry_evidence_writes_attempt_and_blocked_fill -- --nocapture` - PASS;
- `cargo test -p ghost-launcher shadow_v2_validation_smoke_marker_writes_required_artifacts_without_handoff -- --nocapture` - PASS;
- `cargo fmt --check` - PASS.

## D5. Invariants

Zachowane invariants:

- Shadow V2 pozostaje logging-only;
- Shadow V2 records nie sa konsumowane przez decyzje;
- BUY/REJECT nie jest zmieniony;
- Gatekeeper policy nie jest zmieniona;
- selector runtime nie jest zmieniony;
- TX/Jito/live path nie jest zmieniony;
- brak `shadow_close_only`;
- brak active close;
- brak runtime approval;
- brak live-equivalence claim;
- brak R51.

Entry/exit fill bez `pool_state_sample_v2` nie udaje sukcesu. Jest zapisany jako `BLOCKED_BY_DATA`.

## D6. Consequences

Po merge PR18 kolejny validation burnin moze sprawdzic, czy runtime rzeczywiscie produkuje V2-only:

- entry attempt/fill rows;
- path sample rows;
- exit attempt/fill rows;
- terminal truth rows;
- density rows tied to canonical high-watermark;
- replay/lifecycle reconciliation.

PR18 sam nie przyznaje research-grade ani live-equivalence. Jest wymaganym emission layerem przed kolejnym burninem.

## D7. Rejected Alternatives

Odrzucono:

- udawanie executable fill na podstawie legacy lifecycle mark price;
- laczenie terminal truth z legacy exit fill bez pewnego timestamp/source match;
- podlaczenie Shadow V2 do Gatekeeper/selector jako feature path;
- uruchamianie validation burnin w tym PR;
- committowanie raw JSONL/log/runtime artifacts.

## D8. Required Follow-Up

Po review i merge:

1. uruchomic osobny validation/fidelity burnin;
2. wymagac canonical V2 entry/path/exit/terminal rows;
3. wykonac post-run strict manifest audit;
4. wykonac reconstruction/reconciliation/density audit na V2-only evidence;
5. przygotowac report-only PR z verdictem `PASS`, `BLOCKED` albo `FAIL`.

Do tego czasu:

- `research_grade=NOT_GRANTED`;
- `live_equivalence=NOT_GRANTED`;
- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`.
