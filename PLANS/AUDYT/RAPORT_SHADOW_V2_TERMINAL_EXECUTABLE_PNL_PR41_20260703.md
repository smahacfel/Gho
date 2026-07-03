# Raport Shadow V2 PR41: Terminal Executable PnL Wiring

Data: 2026-07-03

Finalny werdykt PR41:

```text
PR41_IMPLEMENTATION_READY_FOR_REVIEW
```

PR41 jest minimalnym implementation PR dla terminal executable PnL. Nie
uruchomiono burnina, validation runu ani zadnego runtime proof. Ten PR nie
nadaje runtime approval, research-grade ani live-equivalence.

Approval flags pozostaja:

```text
RUNTIME_APPROVAL=false
SHADOW_CLOSE_ONLY_APPROVAL=false
ACTIVE_CLOSE_APPROVAL=false
RESEARCH_GRADE=false
LIVE_EQUIVALENCE=false
STRATEGY_RESEARCH_UNBLOCKED=false
BURNIN_AUTHORIZATION=false
```

## 1. Kontekst

PR40 zostal zaakceptowany jako report-only evidence z verdictem:

```text
PR38_EXIT_L1_DIAGNOSTIC_BURNIN_PASS_ROUNDTRIP_BLOCKED
```

Burnin PR38-B potwierdzil, ze realny shadow flow generuje diagnostic L1
entry fill i diagnostic L1 SELL exit fill:

| Metryka PR40 | Wartosc |
|---|---:|
| `accepted_shadow_handoff_count` | 129 |
| `entry_fill_FILLED_count` | 129 |
| `exit_fill_FILLED_count` | 129 |
| `entry_FILLED_exit_FILLED_same_position_count` | 129 |
| `terminal_truth_with_final_pnl_executable_bps_count` | 0 |
| `complete_executable_roundtrip_positions` | 0 |

Problem: mimo ze entry fill i exit fill byly `FILLED` dla tego samego
`position_id`, terminal truth nadal mial:

```text
final_pnl_executable_bps = null
```

## 2. Root cause

Kod odpowiedzialny za terminal truth:

```text
ghost-brain/src/guardian/post_buy/engine.rs
MonitoringEngine::shadow_v2_terminal_truth_from_lifecycle(...)
```

przed PR41:

- zawsze ustawial `final_pnl_executable_bps = None`;
- tworzyl legacy/best-effort `linked_entry_fill`;
- czasem tworzyl syntetyczny `linked_exit_fill`;
- nie wykonywal exact lookupu canonical `shadow_entry_fill_v2` i
  `shadow_exit_fill_v2` po `position_id`;
- nie korzystal z istniejacego helpera:

```text
executable_pnl_bps_from_entry_exit_fills(...)
```

W efekcie terminal truth byl mark-only nawet wtedy, gdy canonical stream mial
FILLED entry i FILLED exit dla tej samej pozycji.

## 3. Zmieniony kod

### 3.1 Helper exact link

Plik:

```text
ghost-brain/src/guardian/post_buy/shadow_v2.rs
```

Dodano:

```text
ShadowV2ExecutablePnlLink
executable_pnl_link_from_canonical_position_fills(...)
```

Helper:

- szuka ostatniego canonical `shadow_entry_fill_v2` dla danego `position_id`;
- akceptuje tylko `fill_status = FILLED`;
- szuka ostatniego canonical `shadow_exit_fill_v2` dla tego samego
  `position_id`;
- dopuszcza pending exit fill z aktualnego appendu, jezeli jeszcze nie zostal
  trwale dopisany do streamu;
- akceptuje pending exit tylko wtedy, gdy ma ten sam `position_id` i
  `fill_status = FILLED`;
- liczy executable PnL przez:

```text
executable_pnl_bps_from_entry_exit_fills(entry_fill, exit_fill)
```

### 3.2 Terminal truth wiring

Plik:

```text
ghost-brain/src/guardian/post_buy/engine.rs
```

Zmieniono:

```text
MonitoringEngine::append_shadow_v2_lifecycle_record(...)
MonitoringEngine::shadow_v2_terminal_truth_from_lifecycle(...)
```

Nowy flow:

1. Lifecycle append buduje exit fill, jezeli record wymaga exit evidence.
2. Jezeli record wymaga terminal truth, przed appendem terminala wykonywany jest
   exact canonical lookup dla entry/exit fills.
3. Jezeli helper znajdzie FILLED entry i FILLED exit dla tego samego
   `position_id`, terminal truth dostaje:

```text
final_pnl_executable_bps = Some(...)
linked_entry_fill = exact canonical entry fill event_id
linked_exit_fill = exact canonical exit fill event_id
reconciliation_status = TERMINAL_TRUTH_WITH_DIAGNOSTIC_EXECUTABLE_PNL
```

4. Jezeli exact link nie istnieje, terminal truth pozostaje mark-only:

```text
final_pnl_executable_bps = None
linked_entry_fill = None
linked_exit_fill = None
reconciliation_status = TERMINAL_TRUTH_FROM_LEGACY_LIFECYCLE_MARK_ONLY
```

## 4. Kontrakt danych

PR41 nie liczy executable PnL z:

- mark price;
- terminal mark PnL;
- replay selected point;
- lifecycle final PnL;
- blocked entry fill;
- blocked exit fill;
- synthetic/best-effort legacy linkow.

Executable PnL moze powstac tylko wtedy, gdy ten sam `position_id` ma:

```text
shadow_entry_fill_v2.fill_status = FILLED
shadow_exit_fill_v2.fill_status = FILLED
```

Wynik pozostaje diagnostic-only:

```text
measurement_grade = DiagnosticOnly
simulation_level = FILL_MODEL_STATIC
```

Terminal truth z executable PnL dostaje limitations:

```text
TERMINAL_EXECUTABLE_PNL_FROM_CANONICAL_ENTRY_EXIT_FILLED_EVENTS
TERMINAL_EXECUTABLE_PNL_DIAGNOSTIC_ONLY_NOT_LIVE_CONFIRMED
```

Terminal truth bez executable linku dostaje typed blockers:

```text
TERMINAL_EXECUTABLE_PNL_BLOCKED_BY_ENTRY_EXIT_FILL_LINK
TERMINAL_ENTRY_FILL_LINK_BLOCKED_BY_CANONICAL_FILL_JOIN
TERMINAL_EXIT_FILL_LINK_BLOCKED_BY_CANONICAL_FILL_JOIN
```

## 5. Testy

Uruchomione testy targeted:

```text
cargo test -p ghost-brain --lib shadow_v2_executable_pnl_link_requires_same_position_filled_entry_and_exit -- --nocapture
cargo test -p ghost-brain --lib shadow_v2_terminal_truth_sets_executable_pnl_from_canonical_filled_entry_and_exit -- --nocapture
cargo test -p ghost-brain --lib shadow_v2_lifecycle_close_emits_path_exit_terminal_records -- --nocapture
```

Wynik:

```text
PASS
```

Testy potwierdzaja:

- exact helper wymaga tego samego `position_id`;
- exact helper wymaga FILLED entry i FILLED exit;
- blocked exit nie daje executable PnL;
- exit dla innej pozycji nie daje executable PnL;
- terminal truth dostaje non-null executable PnL tylko z canonical FILLED
  entry/exit;
- legacy lifecycle close bez executable linku pozostaje mark-only i dostaje
  typed blocker.

## 6. Compile / format checks

Uruchomione:

```text
cargo check -p ghost-brain
cargo check -p ghost-launcher
```

Wynik:

```text
PASS
```

Wystapily istniejace warningi repo, niezwiązane z PR41. Brak nowych compile
errors.

## 7. Granice runtime

PR41 nie zmienia:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- R51;
- `shadow_close_only`;
- active close.

PR41 nie uruchamia:

- burnina;
- validation runu;
- strategy proof;
- edge proof;
- RCE proof.

## 8. Co PR41 nadal nie dowodzi

PR41 jest code-level wiring. Nie jest runtime proof.

Po review i merge wymagany jest osobno zatwierdzony validation/fidelity burnin,
ktory sprawdzi:

- czy `terminal_truth_with_final_pnl_executable_bps_count > 0`;
- czy `complete_executable_roundtrip_positions > 0`;
- czy terminal truth linkuje exact entry/exit fill event ids w realnym scope;
- czy replay/lifecycle reconciliation pozostaje stabilne;
- czy manifest i shutdown nadal sa PASS.

Do tego czasu nadal obowiazuje:

```text
PLAN_PR36_PASS=false
RUNTIME_APPROVAL=false
SHADOW_CLOSE_ONLY_APPROVAL=false
ACTIVE_CLOSE_APPROVAL=false
RESEARCH_GRADE=false
LIVE_EQUIVALENCE=false
STRATEGY_RESEARCH_UNBLOCKED=false
```

## 9. Konkluzja

PR41 naprawia blokade z PR40 na poziomie canonical terminal truth wiring:

```text
entry FILLED + exit FILLED same position
-> exact canonical link
-> diagnostic final_pnl_executable_bps
```

Finalny stan:

```text
PR41_IMPLEMENTATION_READY_FOR_REVIEW
```
