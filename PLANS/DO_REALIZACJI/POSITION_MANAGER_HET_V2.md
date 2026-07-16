# HET-PM V2 — plan implementacji Hierarchical Executable Trajectory Position Manager

## 0. Metryka dokumentu

- Status: `PLAN DO REALIZACJI / POST-PR68 / SHADOW-FIRST`
- Data: `2026-07-16`
- Repo: `smahacfel/Gho`
- Base branch: `main`
- Base SHA: `118df4c205d1c1dee135c03c216b5d7a23c53330`
- Fundament:
  - PR #67 — `Position Manager Lite V1: pure exit policy and safe lifecycle`
  - PR #68 — `Position Manager Lite V1 PR2: max-hold and CrashGuard`
- Zakres aktywacji:
  - najpierw `shadow observe-only`;
  - następnie, po osobnej decyzji opartej o burn-in, `shadow authority`;
  - brak live authority w tym planie.
- Nazwa docelowej architektury: `HEOS-PM`.
- Nazwa pierwszej implementacji: `HET-PM V2`.

---

## 1. Decyzja

Przyjmujemy odchudzoną koncepcję:

> **HET-PM V2 — Hierarchical Executable Trajectory Position Manager**

Jest to deterministyczna, hierarchiczna polityka zarządzania pełną pozycją, oparta o istniejącą kanoniczną trajektorię, leniwe pełnopozycyjne quote’y oraz istniejące kontrakty lifecycle.

Nie jest to jeszcze optimal stopping. Nazwa `HEOS-PM` pozostaje nazwą architektury docelowej, która będzie uzasadniona dopiero po powstaniu wiarygodnej estymacji:

```text
E[value of HOLD | causal trajectory state]
versus
value of EXIT now
```

Pierwszy HET-PM V2 obsługuje wyłącznie:

```text
HOLD
EXIT_ALL
PENDING / RECONCILIATION
TYPED UNKNOWN / BLOCKED
```

Nie obsługuje:

```text
partial exits
capital recovery
ladder exits
runner inventory
continuation ML
wallet clustering
creator-cluster authority
social signals
portfolio opportunity-cost authority
live PumpSwap execution
```

Najważniejsza decyzja architektoniczna:

> **Nie powstaje nowy właściciel pozycji, drugi store, drugi ring buffer ani nowa mikrousługa.**

Docelowy przepływ:

```text
AccountStateCore
      ↓
existing private MonitoredPosition
      ↓
existing bounded SnapshotTimeline
      ↓ pure projection
TrajectoryFeaturesV1 + VitalityFeaturesV1 + RouteEvidenceV1
      ↓
immutable PostBuyDecisionSnapshotV2
      ↓
pure ExitPolicyV2
      ↓
existing sticky proposal
      ↓
existing lazy full-position executable quote
      ↓
existing guarded apply
      ↓
existing typed terminal outcome
      ↓
existing ShadowTerminalTruthV2
```

---

## 2. Dlaczego ten wariant jest właściwy teraz

### 2.1. PR #67 i PR #68 zamknęły najtrudniejszy fundament

Aktualny `main` posiada już:

- jednego kanonicznego właściciela pozycji: `MonitoredPosition`;
- prywatny stan i immutable snapshot;
- `position_id`, `position_epoch`, `state_revision` i remaining quantity guard;
- pure `ExitPolicyV1` bez I/O, RPC, locków i executora;
- sticky `PendingExitProposal` z deterministycznym `action_id`;
- brak drugiego SELL-a dla tej samej epoki;
- leniwy full-position executable quote;
- bounded quote recovery;
- guarded begin/apply/terminal interface;
- jawne `Filled`, `BlockedByData`, `NoFill`, `Failed` i live `Unknown`;
- durable terminal commit;
- bounded direct handoff;
- observation-only Guardian signals;
- AEM i Revolver wyłączone jako aktywne authority;
- absolute max-hold;
- wieloslotowy CrashGuard V1 z raw canonical provenance;
- rozdzielenie mark evidence od executable confirmation.

W związku z tym budowanie osobnego `Position Manager Service`, `Evidence Bus`, Redis/NATS lub drugiego `PositionStore` byłoby regresją architektoniczną.

### 2.2. Istniejąca trajektoria jest wystarczającym nośnikiem pierwszego V2

`SnapshotTimeline` już:

- odrzuca równoważne snapshoty;
- zachowuje slot i timestamp;
- utrzymuje historię ograniczoną czasem i maksymalną liczbą rekordów;
- przechowuje cenę, reserves, market cap, bonding progress i update count;
- wyznacza MFE/MAE;
- zasila CrashGuard;
- zasila TimeStop V2;
- działa w tym samym `MonitoredPosition`.

Retencja jest obecnie wyznaczana jako maksimum wymaganych okien i capped do `2048` rekordów. Przy domyślnym ticku `500 ms` obsługa okien `500 ms`, `1500 ms`, `5 s` i `15 s` nie wymaga drugiego bufora.

### 2.3. Obecne dane wystarczają do polityki trajektoryjnej, ale nie do behavioral authority

Pierwszy HET-PM może uczciwie używać:

- ceny i zwrotów w czasie;
- kanonicznego peaku;
- drawdownu i tempa drawdownu;
- czasu od peaku;
- zmian reserves;
- `reserve_velocity_sol_per_sec` z `AccountStateCore`;
- zmian bonding progress;
- liczby kanonicznych aktualizacji;
- freshness i distinct slots;
- full-position executable quote;
- TimeStop V2 vitality;
- CrashGuard V1.

Nie może jeszcze uczciwie używać jako authority:

- signed OFI;
- buy i sell notional osobno;
- nowych uczestników lub wallet clusters;
- creator-cluster inventory;
- top-holder survival;
- wash-adjusted unique buyers;
- sell acceleration według klas transakcji.

Te dane nie mogą być zastępowane proxy o innej semantyce.

---

## 3. Audyt semantyki obecnych pól — obowiązkowe nazewnictwo

Przed implementacją V2 zamrażamy poniższy kontrakt.

### 3.1. `tx_count` w kanonicznej trajektorii nie jest liczbą transakcji

W `SnapshotTimeline::materialize_canonical_snapshot(...)` pole:

```rust
tx_count: state.update_count
```

reprezentuje liczbę zaakceptowanych aktualizacji `AccountStateCore`, nie udowodnioną liczbę swapów.

Dlatego w `TrajectoryFeaturesV1` nie wolno nazywać tego:

```text
tx_delta_short
transactions_per_second
buy/sell activity
```

Prawidłowa nazwa:

```text
state_update_delta_short
state_update_rate_per_sec
```

z provenance:

```text
source = account_state_core.update_count
semantic_grade = canonical_state_update_activity
```

### 3.2. `cum_volume_sol` jest proxy ruchu rezerw, nie signed volume

Obecna materializacja zwiększa `cum_volume_sol` przez:

```text
abs(current_quote_reserve - previous_quote_reserve)
```

Tracony jest kierunek przepływu, a wartość nie dowodzi realnego traded volume.

W HET-PM V2 nie wolno używać nazw:

```text
volume_delta_short_sol
buy_volume
sell_volume
OFI
```

Prawidłowa nazwa projekcji:

```text
abs_quote_reserve_movement_short_sol
```

z flagą:

```text
semantic_grade = proxy_only_unsigned_reserve_movement
```

Pole może wspierać ocenę vitality, ale nie może udawać order flow.

### 3.3. `unique_addrs` nie jest kanoniczną trajektorią nowych walletów

W kanonicznym materiale pole jest obecnie dziedziczone z poprzedniego snapshotu albo ustawiane na wartość startową. HET-PM V2 nie używa go jako:

```text
unique buyer growth
wallet growth
independent participants
```

### 3.4. `reserve_velocity_sol_per_sec`

To pole pochodzi z `AccountStateCore` i powinno być przekazane do projekcji z jawnym statusem źródła. Nie należy rekonstruować drugiej konkurencyjnej wersji bez potrzeby.

### 3.5. `StatePhase::Migrated`

`StatePhase::Migrated` oznacza obecnie ukończenie curve na podstawie `is_complete`, ale nie oznacza dostępności kanonicznego PumpSwap venue state ani PumpSwap executable route.

W HET-PM V2:

```text
StatePhase::Migrated != PumpSwapReady
```

---

## 4. Twarde granice zakresu

### 4.1. W zakresie

- pure trajectory projection z istniejącego `SnapshotTimeline`;
- immutable `PostBuyDecisionSnapshotV2`;
- pure, hierarchiczny `ExitPolicyV2`;
- reuse CrashGuard V1;
- reuse TimeStop V2 jako vitality projection;
- hard loss potwierdzany full-position executable quote;
- executable trailing z leniwym peak anchor;
- route/data integrity gate;
- absolute max-hold jako ostatni bezpiecznik;
- full exit only;
- observe-only porównanie V1 versus V2;
- shadow authority cutover jako osobny PR po burn-inie;
- pełne provenance, config hash, replay i raport promocji.

### 4.2. Poza zakresem

- PumpSwap state, quote i builder;
- aktywne live execution;
- zmiana live `+58%/-46%`;
- partial quantity accounting;
- realized partial proceeds;
- capital recovery;
- ladder exits;
- runner/moonbag;
- dynamic tip/priority-fee model;
- integer/fixed-point rewrite całego shadow modelu;
- wallet graph i creator clusters;
- signed OFI;
- social data;
- continuation model;
- Type-5 jako authority;
- AEM authority;
- Guardian composite authority;
- Revolver bullets;
- nowy `PositionStore`;
- nowy event broker lub mikrousługa;
- zmiany Gatekeepera, BUY/REJECT/TIMEOUT i prebuy Decision Plane.

---

## 5. Model domenowy

## 5.1. `TrajectoryEvidenceStatus`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TrajectoryEvidenceStatus {
    Complete,
    PartialHistory,
    InsufficientDistinctSlots,
    Stale,
    InvalidOrdering,
    InvalidPrice,
    RouteUnsupported,
    Unavailable,
}
```

Status jest syntetycznym podsumowaniem. Szczegółowe ograniczenia są zachowane w bitowych lub enumowych flagach:

```rust
pub(super) enum TrajectoryEvidenceFlag {
    Return500msUnavailable,
    Return1500msUnavailable,
    Return5sUnavailable,
    Return15sUnavailable,
    ReserveVelocityUnavailable,
    ReserveMovementProxyOnly,
    StateUpdateCountNotTradeCount,
    SameSlotOnly,
    StaleNewestSample,
    ReversedSlotOrdering,
    ReversedTimestampOrdering,
    PumpSwapRouteUnavailable,
}
```

## 5.2. `TrajectoryFeaturesV1`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TrajectoryFeaturesV1 {
    // Causal mark returns.
    return_500ms_bps: Option<i32>,
    return_1500ms_bps: Option<i32>,
    return_5s_bps: Option<i32>,
    return_15s_bps: Option<i32>,

    // Peak and drawdown.
    peak_mark_price_sol: Option<f64>,
    peak_sample_slot: Option<u64>,
    peak_sample_timestamp_ms: Option<u64>,
    drawdown_from_peak_bps: Option<i32>,
    drawdown_velocity_bps_per_sec: Option<i32>,
    time_since_peak_ms: Option<u64>,

    // Canonical reserve / curve state.
    reserve_velocity_sol_per_sec: Option<f64>,
    bonding_velocity_pct_per_sec: Option<f64>,
    state_update_delta_short: Option<u64>,
    state_update_rate_per_sec: Option<f64>,
    abs_quote_reserve_movement_short_sol: Option<f64>,

    // Provenance and quality.
    oldest_sample_slot: Option<u64>,
    oldest_sample_timestamp_ms: Option<u64>,
    newest_sample_slot: Option<u64>,
    newest_sample_timestamp_ms: Option<u64>,
    newest_sample_age_ms: Option<u64>,
    distinct_slots_short: u8,
    status: TrajectoryEvidenceStatus,
    flags: Vec<TrajectoryEvidenceFlag>,
}
```

Zasady:

- brak unbounded history;
- brak sortowania danych z przyszłości;
- brak interpolacji używającej przyszłej próbki;
- brak external RPC;
- brak `.await`;
- brak mutacji pozycji;
- identyczny input + config + `now_ms` daje identyczny wynik.

## 5.3. Materializacja returnów

Dla okna `W`:

1. wybierz najnowszą poprawną raw canonical próbkę `S_now`;
2. wyznacz `target_ts = S_now.ts - W`;
3. wybierz najnowszą próbkę o `sample.ts <= target_ts`;
4. nie używaj próbki po `target_ts`, ponieważ byłaby look-ahead interpolation;
5. wymagaj prawidłowej kolejności slot/timestamp;
6. zapisuj rzeczywisty elapsed time;
7. jeżeli dystans do targetu przekracza limit jakości, zwróć `None` i flagę.

```text
return_W_bps = round(10_000 × (P_now / P_ref - 1))
```

## 5.4. Drawdown velocity

Nie liczymy pochodnej z dwóch runtime-projected timestampów. Używamy raw canonical distinct-slot samples.

Przykładowa projekcja:

```text
dd_now_bps  = 10_000 × (1 - price_now / peak)
dd_prev_bps = 10_000 × (1 - price_prev_distinct_slot / peak)
velocity    = (dd_now_bps - dd_prev_bps) / elapsed_seconds
```

Jeżeli `elapsed_seconds <= 0`, slot jest ten sam albo ordering jest niepoprawny, wartość jest `None`.

## 5.5. `VitalityFeaturesV1`

Nie powstaje drugi time-decay engine. Rozszerzamy istniejący `TimeStopV2State` o czystą projekcję:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum VitalityState {
    Alive,
    Weak,
    HeartbeatOnly,
    StaleOrUnknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct VitalityFeaturesV1 {
    state: VitalityState,
    consecutive_failed_windows: u32,
    last_window_index: Option<u32>,
    last_window_evaluated_at_ms: Option<u64>,
    last_alive_at_ms: Option<u64>,
    candidate_since_ms: Option<u64>,
    last_price_delta_pct_window: Option<f64>,
    last_mcap_delta_pct_window: Option<f64>,
    last_bonding_delta_pct_window: Option<f64>,
    last_state_update_delta_window: Option<u64>,
    last_abs_quote_reserve_movement_sol: Option<f64>,
}
```

Krytyczna zasada:

> V2 nie używa bezpośrednio istniejącego `candidate_emitted` jako authority.

`candidate_emitted` jest historycznym, lepiącym się sygnałem telemetrycznym. Polityka V2 musi używać aktualnego statusu, aktualnej serii nieudanych okien i aktualnej trajektorii recovery. `Alive` resetuje bieżącą serię słabości.

## 5.6. `RouteEvidenceV1`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RouteEvidenceV1 {
    PumpCurveCanonicalSupported,
    CurveCompletePumpSwapUnsupported,
    BootstrapOrPending,
    InvalidOrUnknown,
}
```

Mapowanie pierwszej wersji:

```text
StatePhase::Canonical + valid curve reserves
    -> PumpCurveCanonicalSupported

StatePhase::Migrated lub is_complete=true
    -> CurveCompletePumpSwapUnsupported

StatePhase::Bootstrap/PendingConfirmation
    -> BootstrapOrPending

brak identity / invalid reserves / inconsistent state
    -> InvalidOrUnknown
```

`CurveCompletePumpSwapUnsupported` nie oznacza ekonomicznego `HOLD` ani `CLOSED`.

W observe-only:

```text
V2 candidate = RouteUnsupported
consumed_by_policy = false
```

W przyszłym shadow authority:

```text
bounded route recovery
→ jeśli nadal unsupported:
PositionUnresolved / SimulationBlocked(RouteUnsupported)
→ brak fill i PnL
→ zwolnienie wyłącznie shadow capacity
```

Przed live authority PumpSwap pozostaje osobnym, twardym warunkiem.

## 5.7. `ExecutablePeakAnchorV1`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ExecutablePeakAnchorV1 {
    anchor_id: String,
    position_id: String,
    position_epoch: u64,
    quantity_raw: u64,

    mark_peak_price_sol: f64,
    mark_peak_slot: u64,
    mark_peak_timestamp_ms: u64,

    executable_gross_value_sol: f64,
    conservative_executable_value_sol: f64,
    executable_gross_return_bps: i32,
    conservative_executable_return_bps: i32,

    quote_slot: Option<u64>,
    quote_timestamp_ms: Option<u64>,
    quote_age_ms: Option<u64>,
    quote_source: PriceTruthSource,
    quote_grade: String,

    policy_config_hash: String,
    created_at_ms: u64,
}
```

Anchor jest częścią prywatnego stanu tej samej pozycji, nie drugim store’em.

### Osobny observer guard

W observe-only anchor nie może zmieniać ekonomicznego `state_revision`, ponieważ mogłoby to ingerować w V1 guarded apply.

Dodajemy prywatny observer state:

```rust
struct TrajectoryObserverStateV1 {
    observer_revision: u64,
    executable_peak_anchor: Option<ExecutablePeakAnchorV1>,
    last_observation_key: Option<TrajectoryObservationKey>,
    pending_observation_reservation: Option<PendingTrajectoryObservation>,
}
```

oraz guard:

```rust
struct TrajectoryObserverGuard {
    position_id: String,
    position_epoch: u64,
    quantity_raw: u64,
    source_sample_slot: Option<u64>,
    source_sample_timestamp_ms: Option<u64>,
    observer_revision: u64,
}
```

W PR A:

- observer state nie zmienia quantity;
- nie tworzy pending economic action;
- nie zmienia terminal state;
- nie zwalnia capacity;
- nie zwiększa ekonomicznego `state_revision`;
- nie wpływa na V1 decision.

Po cutoverze PR B ten sam state pozostaje w `MonitoredPosition` i jest chroniony przez jawny guarded interface.

## 5.8. `PostBuyDecisionSnapshotV2`

```rust
pub(super) struct PostBuyDecisionSnapshotV2 {
    guard: PositionSnapshotGuard,
    lane: Lane,

    entry_price_sol: Option<f64>,
    entry_value_sol: Option<f64>,
    entry_quantity_raw: u64,
    remaining_quantity_raw: u64,

    absolute_age_ms: u64,
    mark_price_sol: Option<f64>,
    mark_status: MarkEvidenceStatus,
    mark_source: PriceTruthSource,

    trajectory: TrajectoryFeaturesV1,
    vitality: VitalityFeaturesV1,
    route: RouteEvidenceV1,
    crash: CrashVectorV1,
    executable_peak_anchor: Option<ExecutablePeakAnchorV1>,

    pending_action_id: Option<String>,
    pending_action_reason: Option<ExitReasonV2>,

    policy_id: &'static str,
    policy_version: u16,
    effective_config_hash: String,
    snapshot_id: String,
}
```

Snapshot V2:

- nie zawiera `Arc`, locków, RPC, executora ani mutable collections;
- nie przechowuje referencji do `MonitoredPosition`;
- jest materializowany z tego samego read boundary co snapshot V1;
- zachowuje exact quantity, epoch i revision;
- ma deterministyczny `snapshot_id`.

Aby uniknąć rozjazdu między V1 i V2, engine materializuje jeden bundle:

```rust
struct PostBuyDecisionSnapshotBundle {
    v1: PostBuyDecisionSnapshot,
    v2: PostBuyDecisionSnapshotV2,
    latest_runtime_snapshot: Option<MarketSnapshot>,
    latest_raw_canonical_snapshot: Option<MarketSnapshot>,
    crash_evidence_snapshot: Option<MarketSnapshot>,
}
```

---

## 6. Quote i koszt — ekonomicznie poprawne, ale bez fałszywego net authority

## 6.1. Pełnopozycyjny quote pozostaje istniejący

HET-PM V2 wykorzystuje istniejący:

```text
PriceTruthResolver::resolve_shadow_exit(...)
```

z pełną `remaining_quantity_raw`.

Quote pozostaje oznaczony:

```text
position_sized_curve_executable_gross_costs_unmodeled
```

## 6.2. Conservative cost reserve

Nie modyfikujemy canonical fill truth i nie nazywamy wyniku wiarygodnym net PnL.

Pure policy może używać dodatkowego proxy:

```text
cost_reserve_sol = max(
    configured_floor_sol,
    entry_value_sol × configured_bps / 10_000
)

conservative_executable_value_sol =
    max(0, executable_gross_value_sol - cost_reserve_sol)

conservative_executable_return_bps =
    round(10_000 × (conservative_value / entry_value - 1))
```

Każdy rekord musi zawierać:

```text
net_value_authoritative = false
execution_cost_coverage = conservative_configured_reserve
cost_reserve_model_id
cost_reserve_bps
cost_reserve_floor_sol
```

Nie wolno wpisywać tego proxy do canonical `ShadowTerminalTruthV2.final_pnl_executable_bps` jako authoritative net result.

## 6.3. Lokalna quote cell

W jednym ticku powstaje lokalna, nietrwała komórka:

```rust
struct ExecutableQuoteKey {
    position_id: String,
    position_epoch: u64,
    quantity_raw: u64,
    evidence_source: PriceTruthSource,
    sample_slot: Option<u64>,
    sample_timestamp_ms: Option<u64>,
}
```

Quote może być współdzielony wyłącznie, gdy key jest identyczny.

Nie wolno współdzielić quote’u między:

- runtime-projected baseline sample;
- raw canonical CrashGuard sample;
- inną quantity;
- innym slotem lub timestampem;
- innym evidence source.

Quote nie jest cache’owany między tickami.

## 6.4. Quote budget

HET-PM V2 nie liczy quote’u co `500 ms`.

Quote jest dozwolony tylko dla:

1. nowego meaningful executable peak anchor;
2. emergency CrashGuard candidate;
3. hard-loss mark candidate;
4. trailing mark-drawdown candidate;
5. vitality exit candidate;
6. max-hold candidate;
7. retry istniejącego sticky proposal.

`Hold`, zwykły `Unknown` i brak nowej istotnej peak revision oznaczają zero nowych quote’ów.

---

## 7. Executable peak anchor

## 7.1. Kiedy tworzyć anchor

Anchor jest tworzony lub odświeżany tylko, gdy wszystkie warunki są spełnione:

- route jest `PumpCurveCanonicalSupported`;
- newest raw canonical sample jest świeży;
- sample ma poprawny slot/timestamp ordering;
- nie istnieje pending economic action;
- quantity jest dodatnia;
- mark ustanowił nowy peak;
- oraz zachodzi co najmniej jeden warunek:
  - brak wcześniejszego anchora;
  - nowy peak przekroczył anchor mark peak o `peak_anchor_min_step_bps`;
  - minął `peak_anchor_max_age_ms` i peak nie spadł;
- minął `peak_anchor_min_refresh_ms` od poprzedniego quote’u.

## 7.2. Failure semantics

Jeżeli peak quote jest niedostępny:

- poprzedni anchor pozostaje;
- nie powstaje trailing exit;
- emitowany jest typed `PeakAnchorBlocked`;
- brak fill, close i PnL;
- observation może zostać ponowiona na nowszej revision.

Jeżeli quantity nie zgadza się z anchorem:

- anchor jest nieważny;
- w V2 full-exit-only jest to semantic violation;
- nie wolno porównywać wartości o różnych ilościach.

## 7.3. Comparable executable drawdown

```text
executable_drawdown_bps =
    round(
        10_000 ×
        (anchor.conservative_value - current.conservative_value)
        / anchor.conservative_value
    )
```

Warunki:

- identyczne `quantity_raw`;
- anchor i current quote resolved;
- current quote świeży;
- current quote z tej samej lub nowszej causal revision;
- route zgodny;
- anchor value > 0.

---

## 8. Exit intent i powody

```rust
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ExitIntentV2 {
    Hold,
    ExitAll {
        reason: ExitReasonV2,
        urgency: ExitUrgencyV2,
    },
    Reconcile {
        action_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ExitReasonV2 {
    EmergencyCrash,
    HardExecutableLoss,
    ExecutableTrailing,
    VitalityDecay,
    AbsoluteMaxHold,
    RouteUnsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ExitUrgencyV2 {
    Normal,
    Urgent,
    Emergency,
}
```

`Reconcile` w tym planie jest kontraktem lifecycle, nie nową shadow transakcją. W shadow oznacza:

- istniejący pending proposal;
- pending terminal commit;
- brak nowej decyzji ekonomicznej;
- retry/recovery istniejącej akcji.

---

## 9. Hierarchia HET-PM V2

Kolejność jest leksykograficzna. Pierwsza rozstrzygająca bramka wygrywa.

```text
0. PENDING / RECONCILIATION
1. IDENTITY / DATA / ROUTE INTEGRITY
2. EMERGENCY CRASH
3. HARD EXECUTABLE LOSS
4. ARMED EXECUTABLE TRAILING
5. VITALITY / TIME DECAY
6. ABSOLUTE MAX-HOLD
7. HOLD
```

Polityka może policzyć tanie suppressed candidates dla telemetry, ale quote jest liczony wyłącznie dla winning gate albo dla peak-anchor refresh.

## 9.1. Gate 0 — Pending / reconciliation

Jeżeli istnieje:

- `pending_terminal_commit`;
- `pending_exit_proposal`;
- retryable quote recovery;
- terminal disposition oczekująca na commit;

V2 nie generuje nowej akcji.

Wynik:

```text
Reconcile(existing_action_id)
```

Zachowane inwarianty PR #67/#68:

- jeden sticky `action_id`;
- brak drugiego SELL-a;
- quantity zmienia się dopiero po fillu;
- Unknown nie jest sukcesem;
- stale/duplicate apply nie mutuje pozycji.

## 9.2. Gate 1 — Identity / data / route integrity

Sprawdza:

- lane;
- position ID;
- epoch;
- revision;
- entry price;
- entry i remaining quantity;
- config hash;
- mark provenance;
- route evidence;
- sample ordering;
- freshness.

Wynik niepoprawnych danych:

```text
UnknownEvidenceV2 { typed_reason }
```

Niepoprawne dane:

- nie stają się `Hold`;
- nie tworzą fałszywego close;
- nie tworzą fill/PnL;
- podlegają bounded recovery, jeśli istnieje już proposal.

`CurveCompletePumpSwapUnsupported`:

- PR A: observation-only `RouteUnsupported`;
- PR B: bounded route recovery, następnie shadow unresolved bez PnL;
- nigdy `HOLD_THROUGH_MIGRATION`.

## 9.3. Gate 2 — Emergency Crash

Nie powstaje nowy panic subsystem.

Reuse:

- `CrashVectorV1`;
- raw canonical distinct-slot path;
- current peak;
- freshness;
- existing lazy full-position quote;
- existing `CrashGuardQuoteRequirementV1`;
- existing observation states.

W PR A:

```text
CrashConfirmed
→ V2 candidate EmergencyCrash
→ consumed_by_policy=false
```

W PR B po promocji:

```text
CrashConfirmed
→ ExitAll(EmergencyCrash)
```

Aby nie tworzyć dwóch policy owners, przed PR B neutralne funkcje CrashGuarda należy wydzielić z `exit_policy_v1.rs` do małego modułu:

```text
guardian/post_buy/crash_guard_v1.rs
```

V1 replay i V2 authority korzystają z jednego evaluatora. Tryb `CrashGuardMode::AuthoritativeShadow` nie może działać równolegle z V2 authority.

## 9.4. Gate 3 — Hard executable loss

Hard loss jest dwuetapowy.

### Tani candidate

Candidate powstaje na świeżej mark trajectory, przykładowo po przekroczeniu konfigurowalnego:

```text
mark_return_bps <= hard_loss_mark_candidate_bps
```

Nie jest to jeszcze exit.

### Executable confirmation

Jedno full-position quote wyznacza:

```text
conservative_executable_return_bps
```

Exit candidate jest potwierdzony, gdy:

```text
conservative_executable_return_bps
    <= hard_loss_executable_threshold_bps
```

Wynik zachowuje:

```text
net_value_authoritative=false
```

Nie budujemy w tym PR dynamic fee estimatora ani tip modelu.

## 9.5. Gate 4 — Armed executable trailing

Trailing jest również dwuetapowy.

### Arming

Trailing może być armed dopiero, gdy:

```text
peak_mark_return_bps >= trailing_arm_mark_return_bps
```

oraz istnieje resolved executable peak anchor.

### Mark candidate

Candidate powstaje, gdy:

```text
drawdown_from_peak_bps
    >= trailing_mark_candidate_drawdown_bps
```

### Executable confirmation

Jedno current full-position quote jest porównywane z anchorem.

Exit candidate jest potwierdzony, gdy:

```text
executable_drawdown_bps
    >= trailing_executable_drawdown_bps
```

Nie stosujemy w pierwszym V2 dynamicznego wzoru udającego model kontynuacji. Dystans jest jawnym, wersjonowanym parametrem kalibrowanym przez replay.

## 9.6. Gate 5 — Vitality / time decay

HET-PM korzysta z `VitalityFeaturesV1`, nie z osobnego mechanizmu.

Pierwszy candidate wymaga łącznie:

- current vitality `Weak` lub `HeartbeatOnly`;
- co najmniej `vitality_failed_windows_required` kolejnych nieudanych okien;
- position age >= `vitality_min_age_ms`;
- `time_since_peak_ms >= vitality_min_time_since_peak_ms`;
- brak nowego peak w bieżącej serii;
- brak dodatniej trajectory recovery:
  - `return_1500ms_bps <= vitality_recovery_max_bps`;
  - i/lub `return_5s_bps <= vitality_recovery_max_bps`;
- świeże mark evidence;
- supported route.

`StaleOrUnknown` nie uruchamia time-decay exit. Prowadzi do typed Unknown/Blocked.

Po candidate wykonywany jest full-position quote. Dla vitality wystarcza resolved executable quote i quantity parity; nie ma dodatkowej ręcznie wymyślonej value function.

## 9.7. Gate 6 — Absolute max-hold

Reuse PR #68:

- używa absolute age od entry;
- heartbeat go nie resetuje;
- jest ostatnim bezpiecznikiem;
- full exit only;
- ten sam quote/recovery/apply/terminal contract.

W V2 max-hold jest za vitality, ponieważ nie jest inteligencją strategii, lecz hard occupancy ceiling.

## 9.8. Gate 7 — Hold

`Hold` oznacza:

- wszystkie wcześniejsze bramki zostały prawidłowo ocenione;
- nie ma pending action;
- data i route są wystarczające;
- nie powstał potwierdzony exit candidate.

Brak danych nie może zostać spłaszczony do `Hold`.

---

## 10. Pure `ExitPolicyV2`

Proponowany kontrakt:

```rust
pub(super) struct ExitPolicyV2;

impl ExitPolicyV2 {
    pub(super) fn evaluate_prequote(
        snapshot: &PostBuyDecisionSnapshotV2,
        config: &EffectiveExitPolicyV2Config,
        crash_prequote: &CrashGuardPreQuoteDecision,
    ) -> PreQuoteDecisionV2;

    pub(super) fn finalize_with_quote(
        snapshot: &PostBuyDecisionSnapshotV2,
        candidate: &ExitCandidateV2,
        quote: &ExecutableExitQuote,
        cost_reserve: &ConservativeCostReserveV1,
        config: &EffectiveExitPolicyV2Config,
    ) -> FinalPolicyDecisionV2;
}
```

```rust
pub(super) enum PreQuoteDecisionV2 {
    Hold,
    Reconcile { action_id: String },
    UnknownEvidence { reason: UnknownEvidenceReasonV2 },
    PeakAnchorQuoteRequired { candidate: PeakAnchorCandidateV1 },
    ExitQuoteRequired { candidate: ExitCandidateV2 },
}
```

```rust
pub(super) enum FinalPolicyDecisionV2 {
    Hold,
    UpdatePeakAnchor { anchor: ExecutablePeakAnchorV1 },
    ExitAll { intent: ExitIntentV2 },
    UnknownEvidence { reason: UnknownEvidenceReasonV2 },
}
```

Policy source guard musi zabraniać importów:

```text
tokio
parking_lot
RpcClient
AccountStateReducer
ShadowLedger
MonitoringEngine
executor/backend sender
Instant
```

---

## 11. Konfiguracja V2

Nowa, serde-default sekcja:

```toml
[post_buy_guardian.exit_policy_v2]
mode = "disabled" # PR A profile ustawia observe_only
observation_heartbeat_ms = 5000

return_window_500ms = 500
return_window_1500ms = 1500
return_window_5s = 5000
return_window_15s = 15000
max_reference_gap_ms = 1000
max_newest_sample_age_ms = 1500

cost_reserve_bps_of_entry = 0
cost_reserve_floor_sol = 0.0

hard_loss_mark_candidate_bps = -5000
hard_loss_executable_threshold_bps = -5000

trailing_arm_mark_return_bps = 5000
trailing_mark_candidate_drawdown_bps = 2000
trailing_executable_drawdown_bps = 2000
peak_anchor_min_step_bps = 500
peak_anchor_min_refresh_ms = 1000
peak_anchor_max_age_ms = 10000

vitality_failed_windows_required = 3
vitality_min_age_ms = 11000
vitality_min_time_since_peak_ms = 8000
vitality_recovery_max_bps = 0

route_recovery_ms = 5000
```

Powyższe liczby są jedynie profilem parity/smoke, nie deklaracją optymalnych parametrów.

Właściwe parametry PR B muszą zostać wybrane z replay gridu. Runtime observe-only uruchamia jedną jawną konfigurację; grid jest wykonywany offline, a nie przez wiele równoległych runtime policies.

### Tryb

PR A:

```rust
enum ExitPolicyV2Mode {
    Disabled,
    ObserveOnly,
}
```

PR B dodaje addytywnie:

```rust
AuthoritativeShadow
```

Nie istnieje automatyczna promocja.

### Config hash

`EffectiveExitPolicyV2Config`:

- pełna jawna serializacja `serde_json`;
- BLAKE3;
- policy ID i version w hash input;
- startup status loguje każdy efektywny parametr;
- invalid values blokują start monitora w profilu V2.

---

## 12. Observation contract

## 12.1. Nowy addytywny record type

Do istniejącego `shadow_lifecycle.jsonl` dodajemy:

```text
trajectory_policy_v2_observation
```

Nie powstaje nowy Evidence Bus ani drugi canonical log.

Record zawiera minimum:

```text
position_id / epoch / candidate_id
policy_v1_id/version/hash
policy_v2_id/version/hash
snapshot_id
observer_revision

v1_authoritative_decision
v2_candidate_decision
winning_v2_gate
suppressed_v2_candidates
consumed_by_policy=false

route_evidence
trajectory evidence + flags
vitality features
CrashGuard state

peak anchor identity/value/provenance
current executable quote identity/value/provenance
conservative cost reserve
executable drawdown

quote_requested
quote_resolved
quote_blocker
net_value_authoritative=false
```

Observation nie może zawierać:

```text
exit_landed_slot
PositionClosed
ExitFilled
quantity mutation
terminal PnL
```

## 12.2. Emission policy

Nie emitujemy pełnego rekordu na każdym zwykłym ticku.

Emituj, gdy:

- zmienił się winning gate;
- powstał lub zniknął candidate;
- zmienił się typed blocker;
- utworzono/odświeżono/odrzucono peak anchor;
- zakończono V2 quote;
- wystąpił konflikt V1 versus V2;
- minął observation heartbeat.

Dedupe staje się trwałe dopiero po udanym appendzie, zgodnie z wzorcem PR #68.

Błąd appendu:

- nie mutuje lifecycle;
- czyści krótką reservation;
- pozwala ponowić zapis;
- nie wpływa na V1.

---

## 13. Integracja w ticku — PR A

Kolejność musi gwarantować zero interferencji z V1.

```text
1. refresh existing SnapshotTimeline
2. update canonical peak
3. evaluate existing TimeStopV2 state
4. materialize one snapshot bundle (V1 + V2)
5. run V1 authoritative path bez zmiany semantyki
6. jeśli pozycja nadal aktywna i brak terminal/pending conflict:
     run V2 observe-only
7. emit V2 observation
8. update observer-only peak anchor przez osobny observer guard
```

Dopuszczalna optymalizacja:

- jedna lokalna quote cell może obsłużyć V1 i V2 tylko przy identycznym `ExecutableQuoteKey`;
- w przeciwnym razie V1 ma pierwszeństwo, a V2 nie może zmienić jego evidence source.

V2 observe-only nie może:

- zmienić `state_revision` używanego przez V1;
- utworzyć `PendingExitProposal`;
- zmienić `remaining_token_amount_raw`;
- ustawić close reason;
- usunąć pozycji;
- zwolnić capacity;
- mutować ShadowPositionBook;
- wysłać GuardianSignal jako authority;
- wysłać AEM command;
- zmienić canonical terminal truth.

---

## 14. PR A — Trajectory Policy V2 Observe-Only

## 14.1. Cel

Dostarczyć kompletny causal evidence lane dla HET-PM V2 bez zmiany ekonomicznego zachowania aktywnego shadow managera.

## 14.2. Zmiany kodu

### `ghost-brain/src/guardian/post_buy/trajectory_v1.rs` — nowy

- `TrajectoryFeaturesV1`;
- status i flags;
- pure causal window selection;
- returns;
- drawdown velocity;
- bonding velocity;
- update activity;
- unsigned reserve-movement proxy;
- unit/golden/property tests.

### `ghost-brain/src/guardian/post_buy/exit_policy_v2.rs` — nowy

- config validation;
- config hash;
- snapshot V2;
- route evidence;
- peak anchor types;
- candidate hierarchy;
- prequote/finalize;
- conservative cost reserve;
- source guard tests.

### `ghost-brain/src/guardian/post_buy/engine.rs`

- materializacja snapshot bundle;
- projection z existing timeline;
- `VitalityFeaturesV1` z istniejącego TimeStop V2;
- observer-only state i guard;
- local quote cell;
- peak-anchor observe path;
- V2 observation append/dedupe;
- V1/V2 comparison;
- brak zmiany V1 authority.

### `ghost-brain/src/guardian/post_buy/config.rs`

- `ExitPolicyV2Config`;
- `ExitPolicyV2Mode`;
- safe defaults disabled;
- validation helpers.

### `ghost-brain/src/guardian/post_buy/mod.rs`

- wąski eksport startup statusu V2;
- brak eksportu mutable position state.

### `ghost-launcher/src/components/post_buy_runtime.rs`

- pełne przekazanie configu V2;
- startup validation/status;
- `live_authority=disabled`;
- `v2_consumed_by_policy=false`.

### `ghost-launcher/src/main.rs`

- startup guard:
  - observe-only tylko w shadow;
  - AEM authority disabled;
  - SignalRouter observation-only;
  - Revolver authority disabled;
  - live disabled;
  - required lifecycle path configured.

### Konfiguracja

- default `mode=disabled`;
- osobny rollout profile HET V2 observe-only;
- brak ukrytej zmiany aktywnego profilu.

### Raport/replay

Nowy skrypt:

```text
scripts/analyze_het_pm_v2_observe.py
```

Analizuje:

- V1 versus V2 disagreement;
- candidate counts per gate;
- quote resolution/blocker distribution;
- peak-anchor coverage;
- quote count per position;
- route unsupported rows;
- stale/invalid evidence;
- time-to-candidate;
- counterfactual executable return;
- peak-to-candidate giveback;
- candidate-to-V1-terminal delta;
- max-hold occupancy;
- vitality candidate outcomes;
- CrashGuard overlap;
- no-interference proof.

## 14.3. Inwarianty PR A

- V1 pozostaje jedynym authority;
- identyczne V1 fixtures dają identyczne decisions/outcomes;
- V2 `consumed_by_policy=false` we wszystkich rekordach;
- V2 nie zmienia quantity, action ID, close reason, terminal truth ani capacity;
- zero quote’ów V2 dla zwykłego Hold bez peak refresh;
- najwyżej jeden quote dla identycznego key na tick;
- peak anchor nie używa stale ani runtime-refreshed timestampu;
- same-slot path nie potwierdza crash/trailing velocity;
- route unsupported nie generuje fake fill;
- unsigned reserve movement jest zawsze oznaczone proxy;
- update count nigdy nie jest raportowany jako transaction count;
- brak `.await` pod positions lock;
- bounded timeline pozostaje bounded.

## 14.4. Testy PR A

### Pure trajectory

- exact return przy 500/1500/5000/15000 ms;
- brak look-ahead sample selection;
- insufficient history;
- stale newest sample;
- same-slot samples;
- reversed slot;
- reversed timestamp;
- invalid price;
- deterministic output;
- drawdown velocity boundary;
- bonding velocity;
- update delta semantyka;
- reserve movement proxy flag;
- zero unbounded allocation.

### Pure policy

- pending preempts wszystkie gate’y;
- invalid identity preempts crash;
- route unsupported preempts economic gate’y;
- crash preempts hard loss;
- hard loss preempts trailing;
- trailing preempts vitality;
- vitality preempts max-hold;
- max-hold preempts hold;
- brak danych != Hold;
- candidate bez quote != Exit;
- quantity mismatch blokuje finalization;
- stale quote blokuje finalization;
- anchor quantity mismatch;
- exact trailing boundary;
- exact hard-loss boundary;
- exact vitality failed-window boundary;
- exact max-hold boundary;
- policy import/source guard.

### Engine

- V2 observer nie zmienia V1 state revision;
- V2 observer nie tworzy proposal;
- V1 proposal blokuje V2 new action;
- observation append failure retry;
- observation dedupe po udanym appendzie;
- peak anchor blocked zachowuje poprzedni anchor;
- peak anchor refresh hysteresis;
- no quote spam przy mikropeakach;
- local quote reuse tylko dla identycznego key;
- CrashGuard raw evidence nie jest zastąpione runtime projection;
- TimeStop sticky historical candidate nie staje się V2 vitality authority;
- route unsupported observation bez close/fill;
- shutdown nie tworzy fałszywego terminala.

### No-regression

- pełny Guardian suite;
- V1 policy suite;
- events validator;
- post-buy runtime suite;
- post-buy integration;
- Gatekeeper V2.5/V3;
- replay/logger suites;
- config backward compatibility;
- scoped Clippy;
- rustfmt;
- diff check.

## 14.5. Definition of Done PR A

- [ ] `TrajectoryFeaturesV1` jest pure i causal.
- [ ] Nie powstał nowy store ani runtime owner.
- [ ] Nie powstał drugi ring buffer.
- [ ] `PostBuyDecisionSnapshotV2` jest immutable i prywatny.
- [ ] V2 hierarchy jest pure.
- [ ] V1 jest jedynym authority.
- [ ] V2 nie wpływa na economic `state_revision`.
- [ ] Peak anchor jest lazy i full-position.
- [ ] Hard loss jest executable-confirmed.
- [ ] Trailing jest executable-confirmed.
- [ ] Vitality wykorzystuje istniejący TimeStop V2.
- [ ] Sticky `candidate_emitted` nie jest użyty jako authority.
- [ ] Route unsupported jest typed.
- [ ] Proxy fields mają jawne semantics.
- [ ] Observation records są addytywne i replayable.
- [ ] `consumed_by_policy=false` jest wymuszone testem.
- [ ] V1 parity jest dokładna.
- [ ] Wszystkie wymagane CI są zielone.

---

## 15. Burn-in po PR A

## 15.1. Najpierw baseline PR #68

Przed oceną V2 należy wykonać kontrolowany post-merge burn-in PR #68, aby potwierdzić:

- max-hold działa dokładnie przy 120 s;
- CrashGuard observation ma poprawne raw provenance;
- brak duplicate terminal;
- brak wpływu CrashGuarda na authority;
- quote blockers są jawne;
- canonical terminal truth jest kompletny;
- route/migration rows są policzone osobno.

## 15.2. Burn-in V2

V2 burn-in musi obejmować co najmniej:

- różne launch cohorts;
- różne creator/funder cohorts, jeśli identity jest dostępne do grupowania raportu, ale nie do policy;
- pozycje kończące się targetem, stopem, inactivity, max-hold i unresolved;
- szybkie crashe;
- duże MFE i późniejszy giveback;
- quiet/heartbeat pools;
- curve-complete route-unsupported cases;
- quote blocked i stale evidence cases.

## 15.3. Metryki promocji

### Correctness gates

- duplicate action rate = 0;
- duplicate terminal rate = 0;
- observation interfering with V1 = 0;
- fake fill on blocked route/data = 0;
- unresolved with PnL = 0;
- stale quote accepted = 0;
- anchor/current quantity mismatch accepted = 0;
- proxy semantic mislabel = 0;
- future sample leakage = 0.

### Data quality gates

- trajectory complete rate;
- distinct-slot coverage;
- stale sample rate;
- route-supported share;
- peak-anchor resolution rate;
- current quote resolution rate;
- blocker distribution;
- mean/max quote count per position;
- observation log loss rate.

### Economic comparison

Dla każdego gate’u:

- candidate executable return;
- późniejszy V1 terminal executable return;
- delta candidate versus V1;
- MFE capture ratio;
- peak-to-terminal giveback;
- time saved/lost;
- tail loss distribution;
- CVaR;
- occupancy duration;
- false early-exit proxy: candidate po którym ścieżka osiąga istotnie lepszy executable value;
- missed protection proxy: V1 terminal znacznie gorszy od wcześniejszego V2 candidate.

## 15.4. Replay grid

Offline, nie w aktywnym runtime, testujemy grid:

- hard loss mark candidate;
- hard executable threshold;
- trailing arm;
- mark drawdown candidate;
- executable drawdown threshold;
- peak anchor step/refresh;
- vitality failed windows;
- vitality time-since-peak;
- recovery threshold;
- conservative cost reserve;
- max-hold.

Split raportów:

- per mint;
- per launch cohort/time;
- per creator/funder cohort, jeśli dostępne;
- pre-migration only;
- route-unsupported separately.

Nie wolno stroić parametrów na terminal truth tej samej ścieżki bez purged temporal validation.

## 15.5. Promotion artifact

PR B wymaga jawnego raportu:

```text
HET_PM_V2_SHADOW_AUTHORITY_PROMOTION_REPORT.md
```

Raport zawiera:

- burn-in run IDs;
- config hashes;
- sample counts;
- data completeness;
- V1/V2 comparison;
- tail-risk metrics;
- wybraną konfigurację;
- odrzucone konfiguracje;
- route scope;
- znane limitations;
- operator verdict.

Brak raportu = brak PR B.

---

## 16. PR B — Shadow V2 Authority Cutover

## 16.1. Warunki wejścia

- PR A merged;
- required burn-in complete;
- promotion report zaakceptowany;
- brak correctness blockerów;
- wybrane parametry mają exact config hash;
- PumpSwap nadal nie jest wymagany, ponieważ scope authority jest jawnie pre-migration curve only;
- live pozostaje disabled.

## 16.2. Jeden policy owner

Dodajemy startup mode:

```text
v1_authority
v2_observe_only
v2_authority_shadow
```

W `v2_authority_shadow`:

- tylko V2 może utworzyć economic proposal;
- V1 jest liczony wyłącznie jako replay/baseline observation;
- V1 nie tworzy równoległego proposal;
- CrashGuard nie ma osobnego authority;
- AEM nie ma authority;
- Guardian signals nie mają authority;
- Revolver nie ma authority;
- `ShadowPositionBook` pozostaje mirror.

Nie wolno utrzymywać V1 i V2 jako równoległych produkcyjnych evaluatorów.

## 16.3. Aktywna hierarchia

```text
pending/reconcile
→ data/route
→ CrashGuard confirmed
→ hard executable loss
→ executable trailing
→ vitality decay
→ max-hold
→ hold
```

V2 authority celowo usuwa aktywne V1:

- sztywny take-profit;
- mark-only stop-loss;
- legacy inactivity jako główny time-decay.

V1 pozostaje baseline w logu/replay.

## 16.4. Route unsupported w shadow authority

Jeżeli pozycja przejdzie do `CurveCompletePumpSwapUnsupported`:

1. nie wykonuj curve quote jako gdyby route był nadal wspierany;
2. nie emituj fill;
3. rozpocznij bounded `route_recovery_ms`;
4. jeżeli supported route nadal nie istnieje:
   - `PositionUnresolved`;
   - `SimulationBlocked(RouteUnsupported)`;
   - brak executable PnL;
   - remaining quantity zachowana w recordzie;
   - zwolnienie tylko shadow capacity.

To jest uczciwsze niż sztuczny `HOLD_THROUGH_MIGRATION` albo mark close.

## 16.5. Guarded proposal

Existing proposal/action contract zostaje rozszerzony o:

```text
policy_id = HET-PM V2
winning_gate
trajectory_snapshot_id
vitality_snapshot
route_evidence
peak_anchor_id, jeśli trailing
quote requirement provenance
```

Każdy retry zachowuje original reason i action ID. Nowszy snapshot może dostarczyć świeższy quote, ale nie może tworzyć drugiego action ID.

## 16.6. Terminal truth

Tylko resolved full-position executable quote prowadzi do:

```text
ExitSubmitted
ExitFilled { Filled }
PositionClosed
ShadowTerminalTruthV2
SimulatedClosed
capacity release
```

Brak/stale/invalid truth prowadzi po bounded recovery do:

```text
ExitBlocked
PositionUnresolved
TerminalBlocked
SimulationBlocked
```

bez fill i PnL.

## 16.7. Rollback PR B

Rollback:

```text
mode = v1_authority
```

Kod V2 observation pozostaje, ale nie ma authority. Nie wykonujemy częściowego revertu snapshotów, reason codes lub terminal contractów.

## 16.8. Definition of Done PR B

- [ ] V2 jest jedynym shadow policy ownerem.
- [ ] V1 jest baseline-only.
- [ ] Brak take-profit authority.
- [ ] Hard loss wymaga executable confirmation.
- [ ] Trailing wymaga executable anchor i current quote.
- [ ] Vitality używa bieżącej serii TimeStop V2.
- [ ] Max-hold pozostaje ostatnim bezpiecznikiem.
- [ ] CrashGuard nie jest drugim ownerem.
- [ ] Route unsupported nie daje fake fill.
- [ ] Pending proposal blokuje drugi exit.
- [ ] Shadow unresolved nie ma PnL.
- [ ] Terminal truth pozostaje canonical SSOT.
- [ ] Live pozostaje disabled.
- [ ] Prebuy nie zmienia się.
- [ ] Rollback config jest przetestowany.
- [ ] Pełne wymagane CI jest zielone.

---

## 17. Pliki przewidywane do zmiany

### PR A

```text
ghost-brain/src/guardian/post_buy/trajectory_v1.rs       NEW
ghost-brain/src/guardian/post_buy/exit_policy_v2.rs      NEW
ghost-brain/src/guardian/post_buy/engine.rs
ghost-brain/src/guardian/post_buy/config.rs
ghost-brain/src/guardian/post_buy/mod.rs
ghost-brain/ghost_brain_config.toml
ghost-brain/tests/ghost_brain_config_load_test.rs
ghost-launcher/src/components/post_buy_runtime.rs
ghost-launcher/src/main.rs
ghost-launcher/tests/post_buy_runtime_integration.rs
scripts/analyze_het_pm_v2_observe.py                     NEW
configs/rollout/<het-v2-observe-profile>.toml            NEW
docs/ADR/<HET_V2_PR_A_ADR>.md                            NEW
```

Możliwe addytywne zmiany schema pozostają w istniejącym lifecycle recordzie; nie tworzymy nowej usługi ani store’a.

### PR B

```text
ghost-brain/src/guardian/post_buy/crash_guard_v1.rs      NEW/refactor
ghost-brain/src/guardian/post_buy/exit_policy_v1.rs
ghost-brain/src/guardian/post_buy/exit_policy_v2.rs
ghost-brain/src/guardian/post_buy/engine.rs
ghost-brain/src/guardian/post_buy/config.rs
ghost-brain/src/guardian/post_buy/mod.rs
ghost-brain/ghost_brain_config.toml
ghost-launcher/src/components/post_buy_runtime.rs
ghost-launcher/src/main.rs
ghost-launcher/tests/post_buy_runtime_integration.rs
configs/rollout/<het-v2-authority-shadow>.toml            NEW
docs/ADR/<HET_V2_PR_B_ADR>.md                            NEW
```

---

## 18. Wymagane uruchomienia

Po każdym PR co najmniej:

```bash
cargo test -p trigger entry_price_extractor --quiet
cargo test -p ghost-brain guardian::post_buy --quiet -- --test-threads=1
cargo test -p ghost-brain events::validator --quiet
cargo test -p ghost-brain --test ghost_brain_config_load_test --quiet
cargo test -p ghost-launcher components::post_buy_runtime --quiet -- --test-threads=1
cargo test -p ghost-launcher --test post_buy_runtime_integration --quiet
cargo test -p ghost-launcher --test gatekeeper_v25_regression --quiet
cargo test -p ghost-launcher --test gatekeeper_v3_tests --quiet
cargo check -p ghost-brain --quiet
cargo check -p ghost-launcher --quiet
cargo clippy -p ghost-brain -p ghost-launcher --lib --tests --quiet --message-format short
cargo fmt --all -- --check
git diff --check
```

Dodatkowo:

```bash
python3 -m py_compile scripts/analyze_het_pm_v2_observe.py
```

oraz source/static guards:

- pure policy import guard;
- one authority guard;
- no V2 observation consumption in PR A;
- no live consumption;
- no AEM/Revolver/Guardian authority;
- no partial exit variants;
- no PumpSwap claim.

---

## 19. Kryteria odrzucenia wdrożenia

PR A lub PR B nie może zostać zaakceptowany, jeżeli wystąpi choć jeden z poniższych przypadków:

- V2 zmienia V1 outcome w observe-only;
- observation zwiększa economic state revision i powoduje stale V1 apply;
- dwa action IDs dla jednej epoki;
- route unsupported emituje fill;
- stale evidence uruchamia trailing/crash;
- same-slot samples potwierdzają velocity/crash;
- quote quantity różni się od remaining quantity;
- peak anchor i current quote dotyczą różnych quantities;
- brak danych jest raportowany jako Hold;
- unsigned reserve movement jest raportowany jako volume/OFI;
- update count jest raportowany jako transaction count;
- unresolved posiada final PnL;
- V1 i V2 są równoległymi authority;
- CrashGuard ma osobne authority obok V2;
- AEM/Guardian/Revolver zaczyna mutować pozycję;
- live lane konsumuje V2;
- zmienia się Gatekeeper/prebuy;
- runtime quote jest wykonywany na każdym Hold ticku;
- tick path wykonuje RPC lub await pod positions lock.

---

## 20. Osobne późniejsze inicjatywy

Poniższe elementy nie mogą zostać dopisane „przy okazji” do PR A lub PR B.

### 20.1. PumpSwap Venue Project

Warunki przed live hold-through-migration:

```text
canonical PumpSwap state
+ route identity
+ integer quote
+ matching SELL builder
+ quote/build/simulation parity
+ migration transition tests
```

### 20.2. Partial Exit Project

Wymaga osobno:

```text
exact inventory ledger
realized proceeds
partial fill semantics
fee accounting
amount_raw quote surface
peak rebase
new replay contracts
```

### 20.3. Behavioral Evidence Project

Najpierw kanoniczni producenci:

```text
signed buy/sell notional
wallet identity contract
creator/funder cluster
holder inventory deltas
wash-adjusted participant counts
```

Dopiero potem behavioral authority.

### 20.4. Continuation Model / HEOS

Dopiero po zebraniu causal executable labels:

```text
P(hit upper before lower | trajectory)
future executable return quantiles
hazard of severe executable drawdown
```

Model początkowo observe-only i może modulować parametry, nie przegłosowywać hard safety.

### 20.5. Portfolio Opportunity Cost

Osobny projekt po powstaniu stabilnego queue-quality contractu i portfolio replay.

---

## 21. Ostateczny wynik planu

Po PR A Ghost posiada:

```text
V1 authority
+ causal trajectory V2 evidence
+ executable peak anchors
+ hard-loss candidates
+ executable trailing candidates
+ current vitality candidates
+ route blockers
+ pełne V1/V2 comparison
```

bez zmiany ekonomicznego zachowania.

Po zaakceptowanym burn-inie i PR B Ghost posiada:

```text
jeden deterministic shadow authority:

RECONCILE
  > DATA / ROUTE
  > CRASH
  > HARD EXECUTABLE LOSS
  > EXECUTABLE TRAILING
  > VITALITY DECAY
  > MAX-HOLD
  > HOLD
```

z pełnym exit only, bez ML, partials, drugiego store’a, drugiej mikrousługi i bez fałszywego twierdzenia o optimal stopping.

To jest najmniejsza obecnie zmiana, która może dostarczyć mierzalną poprawę ochrony zysku i tail risk, zachowując wszystkie wartościowe kontrakty PR #67 i PR #68.
