# ADR-8D: Shadow V2 PR41 Terminal Executable PnL Wiring

Data: 2026-07-03

## Status

Accepted for implementation PR, pending review and runtime validation.

Final PR verdict:

```text
PR41_IMPLEMENTATION_READY_FOR_REVIEW
```

## D1. Context

PR40 potwierdzil diagnostic L1 entry fill i diagnostic L1 SELL exit fill w
realnym shadow flow, ale nie potwierdzil executable roundtrip PnL.

Kluczowy blocker:

```text
entry_FILLED_exit_FILLED_same_position_count = 129
terminal_truth_with_final_pnl_executable_bps_count = 0
complete_executable_roundtrip_positions = 0
```

Kod terminal truth w:

```text
ghost-brain/src/guardian/post_buy/engine.rs
MonitoringEngine::shadow_v2_terminal_truth_from_lifecycle(...)
```

ustawial `final_pnl_executable_bps = None` i nie linkowal exact canonical entry
fill oraz exact canonical exit fill.

## D2. Decision

Dodajemy exact canonical executable PnL link:

```text
ghost-brain/src/guardian/post_buy/shadow_v2.rs
ShadowV2ExecutablePnlLink
executable_pnl_link_from_canonical_position_fills(...)
```

Nastepnie terminal truth path uzywa tego linku w:

```text
ghost-brain/src/guardian/post_buy/engine.rs
MonitoringEngine::append_shadow_v2_lifecycle_record(...)
MonitoringEngine::shadow_v2_terminal_truth_from_lifecycle(...)
```

Executable terminal PnL powstaje tylko, gdy:

```text
same position_id
shadow_entry_fill_v2.fill_status = FILLED
shadow_exit_fill_v2.fill_status = FILLED
```

## D3. Invariants

PR41 zachowuje nastepujace invariants:

- brak zmiany BUY/REJECT;
- brak zmiany Gatekeeper policy;
- brak zmiany selector runtime;
- brak zmiany TX/Jito/live path;
- brak dotkniecia R51;
- brak `shadow_close_only`;
- brak active close;
- brak runtime approval;
- brak research-grade;
- brak live-equivalence;
- brak burnina w PR.

## D4. Source Contract

`final_pnl_executable_bps` nie moze byc liczony z:

- mark price;
- terminal mark PnL;
- replay selected point;
- legacy lifecycle final PnL;
- blocked entry fill;
- blocked exit fill;
- best-effort synthetic event ids.

Jedynym inputem jest para canonical fills:

```text
ShadowEntryFillV2(fill_status=FILLED)
ShadowExitFillV2(fill_status=FILLED)
```

dla tego samego `position_id`.

## D5. Pending Exit Fill Handling

Terminal truth moze byc tworzony w tym samym lifecycle appendzie co exit fill.
Dlatego helper przyjmuje `pending_exit_fill`.

To nie jest obejscie canonical streamu:

- pending exit musi miec ten sam `position_id`;
- pending exit musi miec `fill_status = FILLED`;
- terminal linkuje jego docelowy `event_id`;
- append order pozostaje: exit fill przed terminal truth.

## D6. Quality Grade

Terminal truth z executable PnL pozostaje diagnostic:

```text
measurement_grade = DiagnosticOnly
simulation_level = FillModelStatic
quality = TERMINAL_TRUTH_WITH_DIAGNOSTIC_EXECUTABLE_PNL_FROM_CANONICAL_FILLS
```

PR41 nie nadaje:

```text
ResearchGrade
LiveConfirmed
runtime approval
strategy unlock
```

## D7. Blocked Terminal Contract

Jezeli exact canonical link nie istnieje, terminal truth pozostaje mark-only i
emituje typed limitations:

```text
TERMINAL_EXECUTABLE_PNL_BLOCKED_BY_ENTRY_EXIT_FILL_LINK
TERMINAL_ENTRY_FILL_LINK_BLOCKED_BY_CANONICAL_FILL_JOIN
TERMINAL_EXIT_FILL_LINK_BLOCKED_BY_CANONICAL_FILL_JOIN
```

Wtedy:

```text
final_pnl_executable_bps = null
linked_entry_fill = null
linked_exit_fill = null
```

## D8. Tests

Uruchomione testy:

```text
cargo test -p ghost-brain --lib shadow_v2_executable_pnl_link_requires_same_position_filled_entry_and_exit -- --nocapture
cargo test -p ghost-brain --lib shadow_v2_terminal_truth_sets_executable_pnl_from_canonical_filled_entry_and_exit -- --nocapture
cargo test -p ghost-brain --lib shadow_v2_lifecycle_close_emits_path_exit_terminal_records -- --nocapture
cargo check -p ghost-brain
cargo check -p ghost-launcher
```

Wynik:

```text
PASS
```

## D9. Consequences

PR41 usuwa code-level blocker, przez ktory `terminal_truth` nie mogl ustawic
diagnostic executable PnL mimo FILLED entry i FILLED exit dla tej samej pozycji.

Po merge wymagany jest osobno zatwierdzony validation/fidelity burnin. Dopiero
on moze potwierdzic:

- `terminal_truth_with_final_pnl_executable_bps_count > 0`;
- `complete_executable_roundtrip_positions > 0`;
- exact terminal links w realnym runtime scope.

Do czasu takiego burnina obowiazuje:

```text
PLAN_PR36_PASS=false
RUNTIME_APPROVAL=false
SHADOW_CLOSE_ONLY_APPROVAL=false
ACTIVE_CLOSE_APPROVAL=false
RESEARCH_GRADE=false
LIVE_EQUIVALENCE=false
STRATEGY_RESEARCH_UNBLOCKED=false
BURNIN_AUTHORIZATION=false
```
