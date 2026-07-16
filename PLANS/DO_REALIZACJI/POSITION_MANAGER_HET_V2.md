# HET-PM V2 — kompletny plan implementacji Hierarchical Executable Trajectory Position Manager

## 0. Status i źródło prawdy

- Status: `PLAN DO REALIZACJI / SHADOW-FIRST / POST-PR68`
- Data: `2026-07-16`
- Repozytorium: `smahacfel/Gho`
- Branch planu: `agent/het-pm-v2-plan-20260716`
- Fundament:
  - PR #67 — `Position Manager Lite V1: pure exit policy and safe lifecycle`;
  - PR #68 — `Position Manager Lite V1 PR2: max-hold and CrashGuard`.
- Docelowa architektura długoterminowa: `HEOS-PM`.
- Najbliższa implementacja: `HET-PM V2`.
- Ten dokument jest jedynym normatywnym planem HET-PM V2. Nie obowiązują osobne amendmenty.

## 1. Cel nadrzędny i reguła wykonawcza

Ghost ma osiągnąć dodatnie EV przez realną poprawę ekstrakcji edge'u po terminalnym BUY, a nie przez rozbudowę infrastruktury dla samej kompletności architektonicznej.

Obowiązuje reguła:

> Identyfikuj punkt zapalny i root cause. Wprowadzaj najmniejszą zmianę, która realnie rozwiązuje problem i daje mierzalny rezultat. Nie buduj systemu wokół problemu, gdy wystarcza jego bezpośrednia naprawa.

HET-PM V2 ma zastąpić prymitywne zarządzanie oparte głównie na bieżącym TP/SL/time-stop przez deterministyczną politykę, która rozumie kierunek i ewolucję pozycji, ale nadal korzysta z jednego właściciela lifecycle, jednego quote pathu i istniejących terminalnych kontraktów.

## 2. Decyzja architektoniczna

Przyjmujemy:

> **HET-PM V2 — Hierarchical Executable Trajectory Position Manager**

HET-PM V2 jest pełnopozycyjnym, hierarchicznym managerem trajektoryjnym. Nie jest jeszcze prawdziwym optimal stopping, ponieważ nie posiada wyuczonej funkcji wartości:

```text
E[value of HOLD | observed state]
versus
value of EXIT now
```

Pierwsza wersja obsługuje wyłącznie:

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

Najważniejsza decyzja:

> Nie powstaje nowy właściciel pozycji, drugi store, drugi ring buffer, osobny event broker ani mikrousługa Position Managera.

Docelowy przepływ:

```text
AccountStateCore
      ↓
existing private MonitoredPosition
      ↓
existing bounded SnapshotTimeline
      ↓ pure projection
TrajectoryFeaturesV1 + VitalityFeaturesV1
      ↓
RouteStatusV1 + existing CrashGuard evidence
      ↓
immutable PostBuySnapshotBundle
      ↓
pure ExitPolicyV1 + pure ExitPolicyV2
      ↓
existing lazy full-position quote resolver
      ↓
existing sticky proposal / guarded apply
      ↓
existing typed terminal outcome
      ↓
existing ShadowTerminalTruthV2
```

## 3. Co zostaje zachowane z PR #67 i PR #68

Bez zmian pozostają:

- `MonitoredPosition` jako jedyny kanoniczny właściciel pozycji;
- prywatne pola pozycji;
- `position_id`, `position_epoch`, `state_revision` i remaining quantity guard;
- `PostBuyDecisionSnapshot` V1 jako immutable boundary;
- pure `ExitPolicyV1`;
- sticky `PendingExitProposal` i deterministyczne `action_id`;
- jeden pending SELL na epokę;
- brak zmiany quantity przed fill;
- lazy full-position executable quote;
- bounded quote recovery;
- guarded begin/apply/terminal interface;
- durable terminal commit;
- rozróżnienie `Filled`, `BlockedByData`, `NoFill`, `Failed` i live `Unknown`;
- `ShadowTerminalTruthV2` jako canonical terminal SSOT;
- bounded direct handoff;
- Guardian signals jako observation-only;
- AEM i Revolver poza aktywnym authority;
- absolute max-hold;
- wieloslotowy `CrashVectorV1`;
- raw canonical provenance CrashGuarda;
- brak aktywacji live.

HET-PM V2 nie zastępuje tego fundamentu. Rozszerza jedynie sposób oceny pozycji przed istniejącym quote/apply/outcome contractem.

## 4. Charakter trajektorii i granice semantyczne

### 4.1. Trajektoria jest próbkowana, nie kompletna

Aktualny `MonitoringEngine` pobiera podczas ticku najnowszy stan `AccountStateCore`. Jeżeli pomiędzy dwoma tickami wystąpiło kilka account updates, pośrednie ceny i reserves nie trafiają do `SnapshotTimeline`.

Obowiązkowy kontrakt:

```text
trajectory_sampling_mode = latest_canonical_state_per_monitor_tick
measurement_grade = online_non_lookahead_sampled_trajectory
```

Nie wolno opisywać V2 jako:

```text
complete event trajectory
all canonical revisions
fully event-driven path
```

Każdy sample zapisuje:

- monitor tick interval;
- bieżący `state.update_count`;
- delta update count od poprzedniego zapisanego sample;
- `intermediate_updates_unobserved = update_delta > 1`.

### 4.2. `state.update_count` nie jest liczbą transakcji

Pole historycznie materializowane jako `MarketSnapshot.tx_count` reprezentuje liczbę zaakceptowanych aktualizacji `AccountStateCore`, a nie udowodnioną liczbę swapów.

W HET-PM V2 dozwolone nazwy to:

```text
state_update_delta_since_previous_sample
state_update_activity
```

Zakazane nazwy:

```text
transaction_count
transactions_per_second
buy/sell activity
```

Pole jest diagnostyczne i nie jest wejściem pierwszego aktywnego policy V2.

### 4.3. `cum_volume_sol` nie jest volume ani OFI

Obecna materializacja zwiększa tę wartość przez:

```text
abs(current_quote_reserve - previous_quote_reserve)
```

Jest to unsigned proxy ruchu quote reserves. Nie zachowuje kierunku i nie dowodzi realnego traded volume.

HET-PM V2 może logować:

```text
abs_quote_reserve_movement_proxy
```

Nie może używać nazw:

```text
volume
buy_volume
sell_volume
OFI
```

Proxy nie jest wejściem pierwszego aktywnego policy V2.

### 4.4. `unique_addrs` nie jest trajektorią wallet growth

Pole dziedziczone w bieżącej materializacji nie może być używane jako:

```text
unique buyer growth
independent participants
wallet cluster growth
```

### 4.5. Brak look-ahead

Każda projekcja V2 musi być obliczana wyłącznie z próbek dostępnych do `latest_sample_timestamp_ms`. Nie wolno:

- interpolować z późniejszej próbki;
- wybierać próbki znajdującej się po docelowym czasie okna;
- sortować lub naprawiać ścieżki przy użyciu wiedzy z przyszłości;
- używać post-terminalnych danych do decyzji z wcześniejszego ticku.

## 5. Istniejący nośnik trajektorii

`SnapshotTimeline` pozostaje jedynym buforem historii pozycji.

Wymagania:

- brak drugiego ring buffera;
- brak unbounded history;
- brak nowego store'a;
- odrzucanie równoważnych sample pozostaje aktywne;
- retencja obejmuje co najmniej 15 sekund trajektorii oraz istniejące potrzeby CrashGuard/TimeStop/replay;
- limit rekordów pozostaje bounded;
- pure projection nie mutuje timeline;
- projection nie wykonuje RPC, `.await` ani lock acquisition.

## 6. Minimalny model domenowy

### 6.1. `TrajectoryQualityV1`

Route nie jest częścią jakości trajektorii.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TrajectoryQualityV1 {
    Usable,
    PartialHistory,
    InsufficientSamples,
    Stale,
    Invalid,
    Unavailable,
}
```

`Usable` oznacza wystarczające dane dla konkretnej oceny policy. Nie oznacza kompletnego event streamu.

### 6.2. Flagi bez alokacji

Nie używać `Vec<TrajectoryEvidenceFlag>` w hot loopie.

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct TrajectoryFlagsV1(u32);
```

Minimalny zestaw bitów:

```text
RETURN_1500MS_UNAVAILABLE
RETURN_5S_UNAVAILABLE
RETURN_15S_UNAVAILABLE
COLLAPSED_CANONICAL_UPDATES
SAME_SLOT_ONLY
STALE_NEWEST_SAMPLE
INVALID_SLOT_ORDERING
INVALID_TIMESTAMP_ORDERING
INVALID_PRICE
```

Semantyka proxy pól jest stałą cechą schema i nie wymaga powtarzania osobnego wpisu w `Vec` na każdym ticku.

### 6.3. `TrajectoryFeaturesV1`

Pierwsza polityka nie potrzebuje okna 500 ms. Najszybszą warstwą pozostaje istniejący CrashGuard z distinct-slot evidence.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TrajectoryFeaturesV1 {
    // Wieloskalowa ścieżka mark.
    return_1500ms_bps: Option<i32>,
    return_5s_bps: Option<i32>,
    return_15s_bps: Option<i32>,

    // Peak i giveback.
    peak_mark_price_sol: Option<f64>,
    peak_sample_slot: Option<u64>,
    peak_sample_timestamp_ms: Option<u64>,
    drawdown_from_peak_bps: Option<i32>,
    time_since_peak_ms: Option<u64>,
    peak_giveback_velocity_bps_per_sec: Option<i32>,

    // Sampling provenance.
    newest_sample_slot: Option<u64>,
    newest_sample_timestamp_ms: Option<u64>,
    newest_sample_age_ms: Option<u64>,
    distinct_slots_1500ms: u8,
    state_update_delta_since_previous_sample: u64,

    quality: TrajectoryQualityV1,
    flags: TrajectoryFlagsV1,
}
```

Pola log-only, niekonsumowane przez pierwsze policy V2:

```text
reserve_velocity_sol_per_sec
state_update_activity
abs_quote_reserve_movement_proxy
bonding_velocity
```

Mogą zostać zapisane w rozszerzonym evidence recordzie, ale nie powinny zwiększać liczby aktywnych bramek ani progów.

### 6.4. Materializacja returnów

Dla okna `W`:

1. wybierz najnowszy poprawny raw canonical sample `S_now`;
2. wyznacz `target_ts = S_now.timestamp_ms - W`;
3. wybierz najnowszy sample, którego timestamp jest `<= target_ts`;
4. wymagaj poprawnej kolejności slotów i timestampów;
5. wymagaj, aby rzeczywisty elapsed nie przekraczał `W + tolerance`;
6. przy braku jakości zwróć `None` i typed flagę.

```text
return_W_bps = round(10_000 × (P_now / P_ref - 1))
```

Tolerance jest wyprowadzana z monitor ticku:

```text
reference_tolerance_ms = max(2 × monitor_tick_ms, W / 2)
```

### 6.5. Peak giveback velocity

Nie implementować niestabilnej numerycznej drugiej pochodnej.

Pierwsza wersja używa średniej prędkości oddawania od ostatniego kanonicznego peaku:

```text
peak_giveback_velocity_bps_per_sec =
    drawdown_from_peak_bps / max(time_since_peak_ms, 1 ms) × 1000
```

Pole jest ważne tylko, gdy peak i current sample mają prawidłową kolejność oraz pochodzą z tego samego route/modelu.

## 7. Vitality jako bieżący stan, nie sticky historyczny kandydat

HET-PM V2 wykorzystuje istniejący `TimeStopV2State`, ale nie konsumuje bezpośrednio historycznego `candidate_emitted`.

Powstaje immutable projekcja:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum VitalityStateV1 {
    Alive,
    Weak,
    HeartbeatOnly,
    StaleOrUnknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct VitalityFeaturesV1 {
    current_state: VitalityStateV1,
    consecutive_non_alive_windows: u32,
    last_window_at_ms: Option<u64>,
    last_alive_at_ms: Option<u64>,
    latest_window_price_delta_bps: Option<i32>,
    latest_window_state_update_delta: Option<u64>,
    quality_fresh: bool,
}
```

Zasady:

- `Alive` zeruje serię nieudanych okien;
- późniejszy recovery może usunąć bieżący vitality candidate;
- historyczny fakt, że kiedyś wyemitowano candidate, nie jest authority;
- `StaleOrUnknown` nie jest automatycznym EXIT;
- vitality exit wymaga świeżego route i executable quote.

## 8. Route jako osobny kontrakt

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RouteStatusV1 {
    PumpCurveSupported,
    CurveCompletePumpSwapUnsupported,
    Unknown,
}
```

Kontrakt:

```text
StatePhase::Migrated != PumpSwapReady
```

W pierwszym HET-PM:

```text
StatePhase::Migrated
    -> CurveCompletePumpSwapUnsupported
```

Route unsupported nie jest:

- `Hold`;
- trailing breach;
- resolved exit;
- PumpSwap readiness.

W observe-only tworzy typed blocker/candidate. W przyszłym shadow authority prowadzi przez istniejący bounded recovery do unresolved shadow terminalu bez fill i PnL.

Nie wolno porównywać PumpCurve executable anchora z PumpSwap current quote bez osobnego cross-venue continuity contractu.

## 9. Entry capital

Podstawowym ekonomicznym faktem wejścia jest persisted entry amount:

```text
entry_size_lamports / confirmed-or-simulated entry quote amount
```

Nie należy wtórnie liczyć entry capital przez `entry_price × quantity`, jeżeli dokładny amount istnieje.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum EntryValueSourceV1 {
    PersistedEntryAmount,
    DiagnosticPriceTimesQuantityFallback,
    Unavailable,
}
```

Source precedence:

```text
1. persisted entry amount
2. entry price × entry quantity jako jawny diagnostic fallback
3. brak obu -> UnknownEvidence(EntryCapitalUnavailable)
```

Snapshot bundle zawiera:

```rust
entry_value_quote_raw: Option<u64>,
entry_value_source: EntryValueSourceV1,
entry_value_authoritative_for_shadow: bool,
```

PR A nie wprowadza cost reserve ani nie nazywa gross executable return wartością net.

## 10. Executable peak anchor

### 10.1. Cel

Executable trailing nie może porównywać wyłącznie mark price. Potrzebny jest historyczny fakt ekonomiczny: ile pełna bieżąca pozycja była wykonalnie warta na nowym kanonicznym peaku.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ExecutablePeakAnchorV1 {
    position_id: String,
    position_epoch: u64,
    remaining_quantity_raw: u64,

    route_id: String,
    quote_model_id: String,
    quote_state_revision: u64,

    source_snapshot_id: String,
    source_sample_slot: Option<u64>,
    source_sample_timestamp_ms: Option<u64>,
    peak_mark_price_sol: f64,

    executable_value_quote_raw: Option<u64>,
    executable_value_sol: f64,
    executable_gross_return_bps: Option<i32>,

    anchor_seq: u64,
    created_at_ms: u64,
}
```

Jeżeli istniejący resolver nie udostępnia raw output, pole raw pozostaje `None`; PR A nie wykonuje fixed-point rewrite.

### 10.2. Anchor jest historycznym faktem

Anchor może zostać utworzony lub odświeżony wyłącznie, gdy bieżący raw canonical sample ustanawia nowy mark peak.

Nie wolno:

- re-quote'ować anchora na próbce poniżej historycznego peaku;
- przesuwać anchora w dół;
- wygaszać anchora tylko z powodu wieku;
- zastępować anchora current quote'em bez nowego peak eventu.

Refresh:

```text
new canonical mark peak
AND
(
    no existing anchor
    OR new_peak_step_bps >= peak_anchor_min_step_bps
    OR time_since_last_anchor_ms >= peak_anchor_force_refresh_on_new_peak_after_ms
)
```

Ostatni warunek nadal wymaga nowego peaku.

### 10.3. Porównywalność

Anchor jest porównywalny z current quote tylko, gdy zgodne są:

- position ID;
- epoch;
- remaining quantity;
- route ID;
- quote model ID;
- policy config identity;
- brak semantic violation;
- current quote pochodzi z tej samej lub nowszej prawidłowej revision.

Quantity mismatch nie może zostać naprawiony automatycznym skalowaniem.

### 10.4. Observer state

W PR A anchor jest observer-only:

- nie zwiększa ekonomicznego `state_revision`;
- nie tworzy proposal;
- nie zmienia quantity;
- nie wpływa na terminal;
- posiada własny monotoniczny `anchor_seq`;
- apply używa position/epoch/quantity/route/source guard;
- po terminalizacji V1 anchor nie jest już mutowany.

## 11. Snapshot bundle bez duplikacji pól

Nie tworzyć pełnej kopii wszystkich pól V1 w osobnym snapshotcie V2.

```rust
pub(super) struct PostBuySnapshotBundle {
    base: PostBuyDecisionSnapshot,
    v2: PostBuyDecisionExtrasV2,
}

pub(super) struct PostBuyDecisionExtrasV2 {
    trajectory: TrajectoryFeaturesV1,
    vitality: VitalityFeaturesV1,
    route_status: RouteStatusV1,
    executable_peak_anchor: Option<ExecutablePeakAnchorV1>,
    entry_value_quote_raw: Option<u64>,
    entry_value_source: EntryValueSourceV1,
    entry_value_authoritative_for_shadow: bool,
}

pub(super) struct PostBuyDecisionViewV2<'a> {
    base: &'a PostBuyDecisionSnapshot,
    extras: &'a PostBuyDecisionExtrasV2,
}
```

V1 i V2 czytają dokładnie ten sam:

- position ID;
- epoch;
- revision;
- quantity;
- entry identity;
- age;
- mark evidence;
- sample slot/timestamp;
- policy/config identity.

## 12. Polityka HET-PM V2

### 12.1. Hierarchia

```text
0. PENDING / RECONCILIATION
1. IDENTITY / DATA / ROUTE INTEGRITY
2. EMERGENCY CRASH
3. BASELINE HARD LOSS WITH EXECUTABLE CONFIRMATION
4. EXECUTABLE TRAILING
5. RECOVERY-AWARE VITALITY DECAY
6. ABSOLUTE MAX-HOLD
7. HOLD
```

To jest kolejność leksykograficzna. Niższa bramka nie może przegłosować wyższej.

### 12.2. Typy

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum HetPmGateV2 {
    Pending,
    Integrity,
    Crash,
    HardLoss,
    ExecutableTrailing,
    VitalityDecay,
    AbsoluteMaxHold,
    Hold,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum HetPmCandidateV2 {
    Hold,
    Blocked { reason: HetPmUnknownReasonV2 },
    QuoteRequired { reason: HetPmExitReasonV2 },
    SuppressedByPending,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum HetPmFinalDecisionV2 {
    Hold,
    ExitAll { reason: HetPmExitReasonV2 },
    Blocked { reason: HetPmUnknownReasonV2 },
    SuppressedByPending,
}
```

### 12.3. Gate 0 — pending/reconciliation

Jeżeli istnieje pending proposal albo nierozstrzygnięty execution outcome:

```text
V2 -> SuppressedByPending
```

Nie powstaje nowy action ID ani nowy quote request, poza quote retry należącym do istniejącej sticky akcji.

### 12.4. Gate 1 — integrity i route

Sprawdzane są:

- position ID, epoch i revision;
- remaining quantity;
- entry capital availability;
- mark/sample validity;
- sample freshness;
- slot/timestamp ordering;
- route status;
- route/model comparability anchora;
- możliwość zbudowania full-position quote.

Brak danych nie jest `Hold`.

```text
invalid/stale/unsupported
    -> typed Blocked / UnknownEvidence
```

### 12.5. Gate 2 — emergency crash

Reuse istniejącego PR #68:

```text
raw canonical distinct-slot path
+ short-window drop
+ peak drawdown
+ freshness
+ lazy full-position executable quote
```

Nie powstaje drugi CrashGuard ani composite PANIC score.

W PR A CrashGuard zachowuje aktywny effective mode z PR #68, domyślnie `observe_only`.

PR B nie promuje CrashGuarda automatycznie. Jego promocja wymaga jawnego wyniku burn-inu i jawnej zmiany configu.

### 12.6. Gate 3 — hard loss

Hard loss nie jest nowym subsystemem.

V2 wykorzystuje istniejący baseline stop-loss mark candidate. Po jego powstaniu używa tego samego lazy full-position quote'u i zapisuje rzeczywisty gross executable return względem entry amount.

W PR A:

- V1 nadal jest authority;
- V2 nie zmienia progu hard stop;
- nie powstaje drugi stop-loss threshold;
- nie powstaje cost reserve;
- comparison record pokazuje mark return i executable gross return.

### 12.7. Gate 4 — executable trailing

Trailing może zostać uzbrojony dopiero, gdy:

- pozycja osiągnęła skonfigurowany dodatni mark return;
- istnieje porównywalny executable peak anchor;
- route jest wspierany;
- quantity jest zgodna;
- dane są świeże.

Tani prequote candidate:

```text
trailing armed
AND
current mark drawdown from peak >= mark_candidate_drawdown
```

Następnie jeden current full-position executable quote.

Potwierdzenie:

```text
exec_drawdown_bps =
    10_000 × (1 - current_executable_value / anchor_executable_value)

exec_drawdown_bps >= executable_trailing_breach_bps
    -> ExitAll(Trailing)
```

Brak porównywalnego quote'u daje typed blocker, nie trailing exit i nie Hold.

### 12.8. Gate 5 — recovery-aware vitality decay

Vitality candidate wymaga łącznie:

- pozycji starszej niż `vitality_min_age_ms`;
- bieżącego stanu `Weak` lub `HeartbeatOnly`;
- wymaganej liczby kolejnych non-alive windows;
- świeżego trajectory evidence;
- wspieranego route;
- braku nowego peaku przez minimalny czas;
- braku dodatniego recovery w ostatnim oknie 5 s;
- braku wyższego gate'u;
- resolved full-position executable quote przed finalnym EXIT.

Przykładowa reguła recovery:

```text
return_5s_bps >= vitality_recovery_return_bps
    -> vitality exit suppressed for current tick
```

`StaleOrUnknown` nie może samodzielnie wygenerować EXIT.

### 12.9. Gate 6 — absolute max-hold

Reuse PR #68. Max-hold jest ostatnim hard occupancy ceiling, a nie inteligencją managera.

### 12.10. Gate 7 — Hold

`Hold` jest dozwolone wyłącznie po prawidłowej ocenie wcześniejszych gate'ów. Typed blocker nie jest przedstawiany jako Hold.

## 13. Pure evaluation i quote planning

### 13.1. Dwa czyste wyniki

```rust
pub(super) struct HetPmPreQuoteEvaluationV2 {
    candidate: HetPmCandidateV2,
    winning_gate: HetPmGateV2,
    suppressed_gates_mask: u16,
}

pub(super) enum PeakAnchorPreQuoteDecisionV1 {
    NoChange,
    QuoteRequired { source_snapshot_id: String },
    Blocked { reason: HetPmUnknownReasonV2 },
}
```

Peak-anchor observer nie jest drugim policy ownerem. Generuje wyłącznie możliwość materializacji historycznego evidence.

### 13.2. Quote key

Quote może być współdzielony tylko dla identycznego klucza:

```rust
pub(super) struct ExecutableQuoteKeyV2 {
    position_id: String,
    position_epoch: u64,
    state_revision: u64,
    remaining_quantity_raw: u64,
    route_id: String,
    quote_model_id: String,
    sample_slot: Option<u64>,
    sample_timestamp_ms: Option<u64>,
}
```

Wymagania:

- najwyżej jeden resolution na key/tick;
- brak cache między tickami;
- brak quote'u dla zwykłego Hold;
- raw canonical i runtime-projected evidence o innym key nie współdzielą quote'u;
- V2 nie może podmienić V1 quote provenance;
- anchor i exit mogą współdzielić quote tylko przy pełnej identyczności key.

### 13.3. Brak cost modelu w PR A

PR A zapisuje:

- entry amount;
- gross executable value;
- gross executable return;
- quote provenance;
- route/model;
- znane fee/cost components, jeżeli już istnieją.

Analiza offline liczy scenariusze:

```text
gross
gross - fixed floor
gross - 50 bps
gross - 100 bps
gross - 200 bps
```

Dopiero przed PR B zostaje zamrożony ewentualny conservative cost contract. PR A nie nazywa żadnej z tych wartości authoritative net PnL.

## 14. Same-tick V1/V2 comparison boundary

### 14.1. Problem

Jeżeli V1 zostanie zastosowane przed materializacją V2, terminalny tick może usunąć pozycję i spowodować brak V2 evidence dokładnie na najważniejszych obserwacjach.

### 14.2. Normatywna kolejność PR A

```text
1. refresh existing SnapshotTimeline
2. update canonical mark peak
3. evaluate/update existing TimeStopV2 state
4. pod jednym read boundary zmaterializuj PostBuySnapshotBundle
5. oceń pure V1 prequote
6. oceń pure V2 prequote
7. oceń pure peak-anchor request
8. zbuduj lokalny, deduplikowany quote plan
9. rozwiąż potrzebne quote cells bez mutacji pozycji
10. zmaterializuj immutable V1V2ComparisonRecord
11. zastosuj wyłącznie V1 authority przez existing guarded apply
12. zapisz V2 observation z pre-mutation bundle
13. zastosuj observer-only anchor wyłącznie, jeśli pozycja nadal istnieje i guard jest aktualny
```

Inwarianty:

- V1 i V2 widzą ten sam base snapshot;
- V2 nie zwiększa ekonomicznego `state_revision`;
- V2 nie tworzy `PendingExitProposal`;
- V2 nie zmienia V1 guard ani evidence source;
- po terminalizacji nie powstaje nowa V2 ocena ani quote;
- anchor apply ma niższy priorytet niż V1 authority apply.

## 15. Durable comparison evidence bez drugiego terminal systemu

### 15.1. Nonterminal ticks

Nonterminal V2 observations trafiają do jednego bounded sidecara:

```text
het_pm_v2_observations_v1.jsonl
```

Sidecar jest:

- observe-only;
- fail-open względem V1 lifecycle;
- bounded;
- wersjonowany;
- bez osobnego terminal truth;
- bez wpływu na capacity.

### 15.2. Terminal ticks

Na terminalnym ticku immutable V2 comparison zostaje dołączone jako optional nested payload do istniejącego operational terminal recordu przechowywanego w `PendingTerminalCommit`.

Nie powstaje:

- drugi terminal writer;
- drugi commit point;
- nowy canonical terminal SSOT.

Kolejność:

```text
V1 guarded apply
→ existing operational terminal record z optional V2 comparison
→ existing canonical ShadowTerminalTruthV2 append
→ existing cleanup i terminal notification
```

Błąd zapisu V2 części nie może blokować canonical terminal truth. Jeżeli schema operacyjnego recordu nie pozwala na niezależną degradację nested payloadu, record zapisuje typed `v2_comparison_write_status`, a terminal commit pozostaje kontrolowany przez istniejące reguły PR #67.

### 15.3. `V1V2ComparisonRecord`

Minimalne pola:

```rust
pub(super) struct V1V2ComparisonRecord {
    run_id: String,
    lane: Lane,
    position_id: String,
    position_epoch: u64,
    state_revision: u64,
    snapshot_id: String,

    trajectory_sampling_mode: String,
    monitor_tick_ms: u64,

    v1_prequote: String,
    v1_final: Option<String>,
    v2_prequote: String,
    v2_final: Option<String>,
    v2_winning_gate: HetPmGateV2,
    v2_suppressed_gates_mask: u16,
    consumed_by_policy: bool,

    trajectory: TrajectoryFeaturesV1,
    vitality: VitalityFeaturesV1,
    route_status: RouteStatusV1,

    anchor_before: Option<ExecutablePeakAnchorV1>,
    anchor_request: Option<String>,
    anchor_applied: bool,

    quote_keys: Vec<String>,
    quote_resolution_count: u8,
    quote_statuses: Vec<String>,
}
```

`Vec` jest dozwolony w durable record materializowanym poza steady-state policy structem; liczba elementów jest bounded małym limitem wynikającym z quote planu.

## 16. Konfiguracja PR A

Brak sekcji w starym TOML oznacza V2 disabled.

```toml
[post_buy_guardian.het_pm_v2]
enabled = true
mode = "observe_only"

trajectory_short_ms = 1500
trajectory_medium_ms = 5000
trajectory_long_ms = 15000
max_newest_sample_age_ms = 1500

trailing_arm_mark_return_bps = 2500
trailing_mark_candidate_drawdown_bps = 1500
trailing_executable_breach_bps = 1800
peak_anchor_min_step_bps = 500
peak_anchor_force_refresh_on_new_peak_after_ms = 5000

vitality_min_age_ms = 11000
vitality_required_non_alive_windows = 3
vitality_min_time_since_peak_ms = 5000
vitality_recovery_return_bps = 300
```

Powyższe wartości mają status:

```text
C — SAFE INITIAL SHADOW HYPOTHESIS
```

Nie są progami produkcyjnymi i nie uzyskują authority przez sam merge PR A.

Config validation:

- wszystkie okna > 0;
- `short < medium < long`;
- sample age > 0;
- bps w prawidłowych zakresach;
- executable breach nie może być mniejsze od zera;
- anchor step > 0;
- force refresh > 0;
- vitality windows > 0;
- `authoritative_shadow` odrzucone w PR A;
- config hash obejmuje wyłącznie jawne pola HET-PM V2.

Startup record zawiera:

- policy ID/version/hash;
- V2 mode;
- sampling mode;
- trajectory windows;
- trailing hypotheses;
- vitality hypotheses;
- CrashGuard effective mode;
- V1 authority = true;
- V2 authority = false;
- live = disabled.

## 17. Zakres PR A — observe-only

### 17.1. Nowe moduły

```text
ghost-brain/src/guardian/post_buy/trajectory_v1.rs
ghost-brain/src/guardian/post_buy/exit_policy_v2.rs
```

### 17.2. Minimalne rozszerzenia istniejących modułów

- `config.rs` — `HetPmV2Config` i validation;
- `engine.rs` — snapshot bundle, quote plan, anchor observer, comparison emission;
- `exit_policy_v1.rs` — tylko bezpieczne udostępnienie wspólnego base snapshot contractu, bez zmiany V1 ekonomii;
- `events/schema` lub istniejący operational lifecycle record — optional terminal V2 comparison;
- istniejący writer/replay infrastructure — jeden bounded observation sidecar;
- testy i jeden offline analysis script.

### 17.3. Logiczne commity PR A

1. `trajectory projection + snapshot bundle + pure V2 evaluator`;
2. `executable peak anchor + quote planning + V1/V2 comparison durability`.

Nie ma potrzeby tworzenia wielu proceduralnych PR-ów dla każdej klasy lub enumu.

### 17.4. Twarde zakazy PR A

V2 nie może zmienić:

- quantity;
- proposal;
- action ID;
- terminal state;
- capacity;
- close reason;
- canonical terminal truth;
- ekonomicznego `state_revision`;
- V1 config;
- V1 thresholdów;
- BUY/REJECT/TIMEOUT;
- live execution.

## 18. Burn-in i replay

### 18.1. Cel

Burn-in ma odpowiedzieć, czy executable trailing i recovery-aware vitality decay poprawiają ekstrakcję istniejącego entry edge'u bez pogorszenia tail risk i bez zwiększenia problemów wykonawczych.

### 18.2. Obowiązkowe metryki

- coverage trajectory usable/partial/stale;
- udział collapsed updates;
- V1/V2 disagreement rate;
- candidate count per V2 gate;
- suppression reason distribution;
- quote requests na pozycję;
- quote resolution rate;
- quote blockers;
- anchor coverage;
- anchor refresh count;
- executable trailing candidates;
- vitality candidates;
- route blocker distribution;
- candidate executable gross return;
- późniejszy V1 terminal return;
- peak-to-terminal giveback;
- MFE capture ratio;
- MAE i tail losses;
- CVaR;
- occupancy/capital-seconds;
- false early exit proxy;
- missed protection proxy;
- zero duplicate action/terminal violations.

### 18.3. Kontrfaktyczne outcome

Dla każdego V2 exit candidate należy kontynuować observation path po hipotetycznym exit czasie i zapisać co najmniej:

- executable value w candidate time;
- V1 faktyczny terminal value;
- maksymalny późniejszy executable upside;
- maksymalny późniejszy executable downside;
- czas do V1 terminalu;
- giveback uniknięty albo upside utracony;
- route/data availability po candidate.

Nie wolno używać przyszłych danych w samej decyzji. Są dozwolone wyłącznie w offline outcome attribution.

### 18.4. Segmentacja

Raport musi pokazywać wynik osobno co najmniej według:

- terminal reason V1;
- trajectory quality;
- anchor coverage;
- route status;
- age bucket;
- entry cohort/time cohort;
- creator/funder cohort, jeżeli stabilna tożsamość jest dostępna wyłącznie do splitu, nie do policy.

Długie ścieżki nie mogą dominować wyniku tylko dlatego, że produkują więcej ticków. Jednostką podstawową jest pozycja/epoka.

## 19. Promotion gates przed PR B

Nie istnieje automatyczna promocja.

### Gate 1 — lifecycle integrity

Wymagane:

```text
duplicate action = 0
duplicate terminal = 0
V2 economic mutation in observe-only = 0
V2 proposal creation = 0
route/build authority changes = 0
```

### Gate 2 — data coverage

- trajectory coverage jest zmierzona;
- collapsed updates są raportowane;
- anchor coverage jest wystarczająca dla oceny trailing;
- route blockers są typed;
- quote blocker rate jest znany;
- brak niejawnego zamieniania braków na Hold.

Nie zamrażamy arbitralnego procentu w tym planie. Raport musi jednak wykazać, że decyzja nie opiera się na małej, selektywnej podpróbie.

### Gate 3 — quote budget

- zwykły Hold nie wykonuje quote;
- jeden quote per identical key/tick;
- brak cache między tickami;
- anchor nie jest quote'owany na każdym mikropeaku;
- quote count per position jest bounded i zaakceptowany.

### Gate 4 — wynik ekonomiczny

Co najmniej:

- executable trailing zmniejsza peak-to-terminal giveback;
- vitality decay zmniejsza occupancy albo terminal loss;
- CVaR/tail loss nie pogarsza się istotnie;
- poprawa nie wynika wyłącznie z kilku ekstremalnych pozycji;
- wynik pozostaje dodatni po realistycznych offline cost scenarios.

### Gate 5 — stabilność

- więcej niż jedna sesja/run;
- więcej niż jeden launch cohort;
- brak dominacji jednego twórcy/fundera;
- stabilny kierunek efektu w głównych segmentach;
- brak causal/data contract violations.

## 20. PR B — shadow authority cutover

PR B powstaje dopiero po zaakceptowanym raporcie promocji.

### 20.1. Cutover

```text
V2 = jedyny shadow policy owner
V1 = baseline/replay observation only
```

V1 po cutoverze nie może:

- tworzyć proposal;
- wykonywać guarded apply;
- terminalizować pozycji;
- zwalniać capacity.

### 20.2. Gate-specific promotion

PR B jawnie określa, które mechanizmy uzyskują authority:

- hard loss pozostaje istniejącym ochronnym kontraktem;
- executable trailing może zostać promowany;
- vitality decay może zostać promowany;
- CrashGuard pozostaje zgodny z własnym effective mode i nie jest promowany automatycznie;
- max-hold pozostaje hard ceiling.

### 20.3. Cost contract

Przed PR B należy wybrać jeden jawny sposób konserwatywnej oceny execution costs albo pozostawić decyzję na gross executable value z osobnym safety margin w thresholdzie.

Wybrany kontrakt musi:

- być wersjonowany;
- być częścią config hash;
- nie nazywać proxy authoritative net PnL;
- mieć replay dla kilku scenariuszy kosztowych;
- nie zmieniać historycznych rekordów PR A.

### 20.4. Nadal poza PR B

- live;
- partial exits;
- ML;
- PumpSwap;
- AEM;
- Revolver;
- wallet graph;
- behavioral authority;
- portfolio opportunity cost.

## 21. Testy PR A

### 21.1. Trajectory projection

- identyczna timeline + `now_ms` -> identyczny wynik;
- brak próbki po target timestamp;
- 1500 ms return boundary;
- 5 s return boundary;
- 15 s return boundary;
- zbyt stary reference -> `None`;
- same-slot-only -> typed flag;
- reversed slot ordering -> Invalid;
- reversed timestamp ordering -> Invalid;
- invalid price -> Invalid;
- multiple updates między tickami ustawiają collapsed flag i exact delta;
- sampled path nigdy nie raportuje complete event trajectory;
- `return_500ms` nie istnieje w contract.

### 21.2. Peak/giveback

- peak tylko rośnie;
- non-peak sample nie zmienia peaku;
- time since peak jest monotoniczny do nowego peaku;
- peak giveback velocity ma prawidłowy znak i denominator;
- brak peaku -> brak drawdownu;
- nowy peak resetuje drawdown.

### 21.3. Executable anchor

- pierwszy nowy peak tworzy anchor request;
- non-peak sample nie refreshuje anchora;
- anchor nigdy nie przesuwa się w dół;
- stary anchor nie wygasa tylko z powodu czasu;
- mały nowy peak respektuje min step;
- małe kolejne highs mogą refreshować dopiero po force interval, nadal tylko na nowym peaku;
- quantity mismatch odrzuca porównanie;
- epoch mismatch odrzuca apply;
- route mismatch odrzuca porównanie;
- quote model mismatch odrzuca porównanie;
- stale observer apply nie mutuje anchora;
- terminalizacja V1 pomija anchor apply.

### 21.4. Vitality

- Alive zeruje consecutive failures;
- Weak zwiększa serię;
- HeartbeatOnly zwiększa serię;
- StaleOrUnknown nie tworzy samodzielnego exit;
- recovery suppressuje vitality candidate;
- nowy peak suppressuje vitality candidate;
- zbyt młoda pozycja nie tworzy candidate;
- brak świeżych danych daje blocker.

### 21.5. Pure policy hierarchy

- pending preemptuje wszystko;
- integrity preemptuje crash/trailing/vitality;
- crash preemptuje niższe gate'y zgodnie z effective mode;
- hard stop przed trailing;
- trailing przed vitality;
- vitality przed max-hold;
- max-hold przed Hold;
- typed blocker nie staje się Hold;
- identyczny bundle/config -> identyczny result.

### 21.6. Quote planning

- Hold -> zero quote;
- anchor-only tick -> najwyżej jeden quote;
- trailing candidate -> jeden current quote;
- V1 i V2 same key -> jedno resolution;
- different key -> oddzielne resolution;
- brak cache między tickami;
- full remaining quantity zawsze częścią key;
- V2 nie zmienia V1 quote source.

### 21.7. Same-tick boundary

- terminalny V1 tick nadal posiada V2 comparison z pre-mutation bundle;
- comparison używa pre-V1 quantity i revision;
- brak nowego V2 quote po terminalizacji;
- V2 precomputation nie zmienia V1 guard;
- V1 apply ma pierwszeństwo przed anchor observer apply;
- terminal operational record może zawierać optional V2 comparison;
- V2 write degradation nie fałszuje canonical terminal truth.

### 21.8. Observe-only isolation

- dowolny V2 result nie zmienia quantity;
- nie tworzy proposal;
- nie zmienia action ID;
- nie zmienia close reason;
- nie zmienia terminal truth;
- nie zwalnia capacity;
- nie zwiększa economic state revision;
- nie aktywuje live.

## 22. Testy PR B

Poza wszystkimi testami PR A:

- V2 jest jedynym shadow apply ownerem;
- V1 nie tworzy proposal;
- V1 terminal side effects = 0;
- trailing resolved quote prowadzi do jednego full exit;
- vitality resolved quote prowadzi do jednego full exit;
- stale/blocked quote używa istniejącego recovery contractu;
- unresolved nie emituje fill ani PnL;
- duplicate action = 0;
- route unsupported kończy shadow zgodnie z typed unresolved contractem;
- rollback do V1 wymaga pełnego revertu PR B, bez dual authority.

## 23. Minimalne uruchomienia CI

PR A i PR B muszą uruchamiać co najmniej:

```bash
cargo test -p ghost-brain guardian::post_buy
cargo test -p ghost-brain events::validator
cargo test -p ghost-launcher components::post_buy_runtime
cargo test -p ghost-launcher --test post_buy_runtime_integration
cargo test -p trigger entry_price_extractor
cargo test -p ghost-launcher --test gatekeeper_v25_regression
cargo test -p ghost-launcher --test gatekeeper_v3_tests
cargo fmt --all -- --check
git diff --check
```

Dodatkowo:

- nowe focused trajectory/policy/anchor tests;
- logger/replay suites;
- Shadow V2 lifecycle suites;
- diff-scoped Clippy dla zmienionych plików/crates;
- forbidden-scope guard dla prebuy i live execution.

## 24. Świadomie poza zakresem

Ten plan nie implementuje:

- nowego `PositionStore`;
- nowego trajectory service;
- drugiego ring buffera;
- Redis/NATS/event brokera;
- partial exits;
- exact capital recovery;
- ladderów;
- runnera;
- dynamicznego trailing modelowanego ML;
- continuation modelu;
- wallet clustering;
- creator/insider authority;
- signed OFI;
- social;
- portfolio governor;
- dynamic Jito tip model;
- PumpSwap snapshot/quote/builder;
- live authority;
- AEM authority;
- Revolver bullets;
- pełnego fixed-point rewrite;
- zmian Gatekeepera V2/V2.5/V3;
- zmian BUY/REJECT/TIMEOUT;
- Type-5 authority.

Każdy z tych tematów jest oddzielną przyszłą inicjatywą i wymaga własnego uzasadnienia przyrostowej wartości.

## 25. Rollback

### PR A

Rollback jest pełnym revertem PR A.

Ponieważ V2 jest observe-only:

- V1 pozostaje aktywnym authority;
- brak migracji pozycji;
- brak zmiany terminal truth;
- brak zmiany quantity;
- sidecar może zostać po prostu wyłączony/usunięty.

Nie należy pozostawiać połowy snapshot schema ani nieużywanych observer states.

### PR B

Rollback jest pełnym revertem authority cutover.

Nie wolno utrzymywać V1 i V2 jako równoległych apply owners „na okres migracji”. Po rollbacku V1 wraca jako jedyny authority, a V2 może pozostać wyłącznie observe-only, jeśli dokładnie taki stan zapewnia pełny revert/cutover config.

## 26. Definition of Done — PR A

- [ ] istnieje jeden normatywny plan HET-PM V2;
- [ ] brak osobnych amendmentów;
- [ ] nie powstał drugi owner/store/buffer;
- [ ] trajectory sampling jest jawnie `latest_canonical_state_per_monitor_tick`;
- [ ] nie istnieje `return_500ms`;
- [ ] route jest oddzielony od trajectory quality;
- [ ] hot-path flags nie używają `Vec`;
- [ ] proxy update/reserve fields nie są authority;
- [ ] `TrajectoryFeaturesV1` ma wyłącznie 1.5 s / 5 s / 15 s returns, peak, drawdown, time since peak i giveback velocity;
- [ ] `VitalityFeaturesV1` reprezentuje bieżący stan, nie sticky historyczny candidate;
- [ ] entry amount ma pierwszeństwo przed price × quantity;
- [ ] snapshot V2 nie duplikuje base pól V1;
- [ ] executable anchor powstaje wyłącznie na nowym peaku;
- [ ] anchor nie przesuwa się w dół i nie wygasa tylko przez wiek;
- [ ] quote key jest jawny i deduplikowany per tick;
- [ ] Hold nie uruchamia quote;
- [ ] PR A nie posiada cost reserve;
- [ ] V1 i V2 widzą ten sam pre-mutation bundle;
- [ ] terminalny V1 tick zachowuje V2 comparison w existing terminal flow;
- [ ] V2 nie mutuje lifecycle ani economic revision;
- [ ] V1 pozostaje jedynym authority;
- [ ] live pozostaje disabled;
- [ ] pełne wymagane CI jest zielone.

## 27. Definition of Done — PR B

- [ ] istnieje zaakceptowany burn-in report;
- [ ] promotion gates są spełnione;
- [ ] jawnie wskazano promowane gate'y;
- [ ] V2 jest jedynym shadow authority;
- [ ] V1 jest baseline/replay only;
- [ ] V1 nie tworzy proposal ani terminalu;
- [ ] istnieje jeden sticky action/apply owner;
- [ ] brak duplicate action/terminal;
- [ ] trailing i vitality używają existing quote/recovery/outcome contractu;
- [ ] typed blocker nie staje się Hold;
- [ ] unresolved nie emituje fill/PnL;
- [ ] CrashGuard nie został niejawnie promowany;
- [ ] live, partials, ML i PumpSwap pozostają poza zakresem;
- [ ] pełne wymagane CI jest zielone.

## 28. Ostateczny rezultat

Po PR A Ghost posiada:

```text
real sampled trajectory
+ pure hierarchical V2 candidate policy
+ historical executable peak anchor
+ executable trailing evidence
+ recovery-aware vitality evidence
+ exact same-tick V1/V2 comparison
+ zero economic behavior change
```

Po zaakceptowanym PR B Ghost posiada:

```text
one canonical full-position shadow manager
+ existing emergency/loss/max-hold safety
+ executable trailing
+ recovery-aware vitality decay
+ existing guarded execution lifecycle
```

Jest to najmniejszy krok, który realnie zmienia Position Manager z prostego automatu TP/SL/time-stop w trajektoryjny manager pozycji, bez jednoczesnego budowania HEOS-PM, PumpSwap execution, partial inventory systemu i programu ML.
