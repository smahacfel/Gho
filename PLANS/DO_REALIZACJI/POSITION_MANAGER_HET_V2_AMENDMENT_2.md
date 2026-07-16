# HET-PM V2 — Amendment 2: sampling, entry capital and historical peak-anchor semantics

Status: `NORMATIVE CLARIFICATION`

Ten dokument doprecyzowuje kontrakty trajektorii i executable peak anchora w `POSITION_MANAGER_HET_V2.md`.

## 1. SnapshotTimeline jest trajektorią próbkowaną, nie pełnym event streamem

Aktualny `MonitoringEngine` odczytuje z `AccountStateCore` najnowszy stan podczas ticku i dopiero wtedy dołącza go do `SnapshotTimeline`.

Oznacza to:

- pierwsza wersja HET-PM pracuje na latest-state-per-monitor-tick;
- wiele canonical updates pomiędzy tickami może zostać skompresowanych do jednego punktu;
- `state.update_count` delta ujawnia, że nastąpiło więcej aktualizacji, ale nie odtwarza ich pośrednich cen ani reserves;
- nie wolno opisywać pierwszej wersji jako „każda canonical revision” ani jako pełny event-driven path.

Do evidence flags należy dodać:

```rust
CanonicalUpdatesCollapsed {
    update_delta: u64,
}
```

oraz logować:

```text
trajectory_sampling_mode = latest_canonical_state_per_monitor_tick
monitor_tick_ms
state_update_delta_since_previous_sample
intermediate_updates_unobserved = state_update_delta > 1
```

Returns, drawdown velocity i bonding velocity pozostają causal dla zaobserwowanej trajektorii, lecz ich measurement grade brzmi:

```text
sampled_canonical_trajectory
```

Nie `complete_event_trajectory`.

Ewentualny event-driven accumulator każdej revision jest osobnym późniejszym projektem i nie należy do PR A/B.

## 2. Executable peak anchor jest historycznym faktem

Anchor może zostać utworzony lub odświeżony wyłącznie na raw canonical sample, który sam ustanawia nowy mark peak dla pozycji.

Nie wolno:

- re-quote’ować anchora na bieżącej próbce znajdującej się poniżej historycznego peaku;
- przesuwać anchora w dół z powodu wieku;
- traktować historycznego anchora jako stale tylko dlatego, że minął czas;
- zastępować anchora current quote’em bez nowego peak eventu.

Prawidłowy refresh:

```text
current sample establishes new canonical mark peak
AND
(
    no anchor
    OR new_peak_step_bps >= peak_anchor_min_step_bps
    OR time_since_last_anchor_ms >= peak_anchor_force_refresh_on_new_peak_after_ms
)
```

Ostatni warunek nadal wymaga **nowego peaku**. Pozwala zakotwiczyć serię małych kolejnych highs bez quote’u na każdym mikropeaku.

Pole konfiguracyjne:

```text
peak_anchor_max_age_ms
```

należy zastąpić przez:

```text
peak_anchor_force_refresh_on_new_peak_after_ms
```

Historyczny anchor pozostaje ważny do porównania, dopóki zgodne są:

- position ID;
- epoch;
- quantity;
- route/model identity;
- policy config hash;
- quote provenance nie ma semantic violation.

## 3. Entry capital source

Dla full-position HET-PM podstawowym entry capital jest immutable ekonomiczny fakt wejścia:

```text
entry_size_lamports / confirmed-or-simulated entry fill amount
```

Nie wtórne przemnożenie mark entry price przez quantity, jeżeli dokładny entry amount jest dostępny.

Source precedence:

```text
1. persisted entry fill quote amount / entry_size_lamports
2. entry_price × entry_quantity wyłącznie jako jawny diagnostic fallback
3. brak obu źródeł -> UnknownEvidence(EntryCapitalUnavailable)
```

Snapshot V2 musi zawierać:

```rust
entry_value_sol: Option<f64>,
entry_value_source: EntryValueSourceV1,
entry_value_authoritative_for_shadow: bool,
```

Conservative executable return jest liczony względem tego samego immutable entry capital dla całego full-position lifecycle.

## 4. Route/model comparability

Nie wolno porównywać:

```text
PumpCurve executable peak anchor
```

z:

```text
PumpSwap current executable quote
```

bez osobnego, jawnego cross-venue continuity contractu.

W tym planie:

```text
anchor.route_id != current_quote.route_id
    -> RouteUnsupported / UnknownEvidence
```

Nie trailing breach i nie Hold.

Migration invaliduje możliwość użycia curve anchora do aktywnej decyzji po migracji, ale nie usuwa historycznego evidence z logu.

## 5. Dodatkowe testy

- `multiple_account_updates_between_ticks_set_collapsed_updates_flag`;
- `sampled_returns_never_claim_complete_event_path`;
- `non_peak_sample_never_refreshes_executable_anchor`;
- `old_anchor_does_not_expire_only_due_to_wall_clock_age`;
- `small_new_peaks_refresh_only_after_force_interval`;
- `anchor_never_moves_downward`;
- `entry_fill_amount_precedes_price_times_quantity`;
- `entry_value_fallback_is_diagnostic_and_explicit`;
- `missing_entry_capital_returns_typed_unknown`;
- `cross_route_anchor_quote_comparison_is_rejected`.
