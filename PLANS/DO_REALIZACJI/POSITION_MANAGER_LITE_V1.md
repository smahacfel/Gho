# Position Manager Lite V1 — plan wykonawczy

## 1. Cel i wynik docelowy

Celem jest zbudowanie spójnego, bezpiecznego Position Managera dla aktywnego shadow lifecycle bez tworzenia nowego `PositionStore`, bez reaktywacji AEM/Revolvera i bez przebudowy dormant live lane.

Plan powstaje na aktualnej zawartości `main` odpowiadającej merge commitowi `53382696eb06affbd309ca4d050f030d31a561b0`. Lokalny HEAD ma inną historię, ale `git diff HEAD origin/main` jest pusty, a working tree czysty.

Docelowa ścieżka:

```text
AccountStateCore
      ↓
private MonitoredPosition
      ↓
immutable PostBuyDecisionSnapshot
      ↓
pure ExitPolicyV1::evaluate_prequote()
      ↓ tylko gdy powstał kandydat exit
lazy full-position executable quote
      ↓
pure ExitPolicyV1::finalize_with_quote()
      ↓
guarded begin/apply interface
      ↓
shadow executor
      ↓
typed execution outcome
      ↓
canonical Shadow V2 terminal truth
      ↓
typed terminal notification
      ↓
shadow slot release
```

Najważniejsze decyzje:

- `MonitoredPosition` pozostaje jedynym kanonicznym właścicielem aktywnego stanu shadow.
- Nie powstaje drugi store ani nowy równoległy model pozycji.
- Polityka nie czyta locków, runtime’u, RPC ani mutable state.
- Polityka nie wykonuje sprzedaży i nie modyfikuje pozycji.
- Wszystkie pola pozycji i snapshotu pozostają prywatne.
- Odczyt odbywa się przez immutable snapshot, a mutacja tylko przez kontrolowane metody `begin/apply`.
- Executable quote jest liczony leniwie, tylko po wykryciu realnego kandydata exit lub kontrfaktycznego CrashGuard candidate.
- Aktywne progi +50%, −50% i 30 s inactivity pozostają bez zmian.
- Absolute max-hold wynosi 120 s i zostaje aktywowany wyłącznie w shadow w PR 2.
- CrashGuard w PR 2 działa początkowo wyłącznie `observe_only`.
- Shadow i live otrzymują różne, jawne znaczenie `Unknown`.
- Kanonicznym terminalnym SSOT pozostaje istniejący `ShadowTerminalTruthV2`; nowe eventy operacyjne są tylko jego projekcjami.
- Live execution pozostaje wyłączone.

Plan jest podzielony na dwa większe PR-y.

---

## 2. Kontrakty architektoniczne i typy

### 2.1 Prywatny stan i immutable snapshot

`MonitoredPosition` pozostaje prywatnym typem wewnątrz warstwy post-buy. Nie należy udostępniać jego pól launcherowi, policy ani executorowi.

Do pozycji należy dodać wyłącznie stan potrzebny do bezpiecznego lifecycle:

- monotoniczny `state_revision`;
- `next_exit_action_seq`;
- opcjonalny `PendingExitProposal`;
- opcjonalny sender terminalnego wyniku shadow;
- peak aktualizowany na podstawie poprawnych kanonicznych snapshotów;
- ostatni zastosowany action ID;
- status ostatniego outcome.

Nowy `PostBuyDecisionSnapshot` ma prywatne pola i konstruktor dostępny tylko wewnątrz `guardian::post_buy`. Snapshot zawiera:

- `position_id`;
- `position_epoch`;
- `state_revision`;
- lane;
- entry price i entry raw quantity;
- remaining raw quantity;
- entry/opened timestamp;
- absolute age;
- inactivity age;
- bieżącą poprawną mark price lub typed evidence status;
- latest slot i timestamp;
- provenance źródła;
- peak price i drawdown;
- zwarty short-window crash vector;
- informację o istniejącym pending proposal;
- policy ID i effective config hash.

Snapshot nie może zawierać:

- referencji do `MonitoredPosition`;
- `Arc<RwLock<...>>`;
- `Instant`;
- klienta RPC;
- `AccountStateReducer`;
- `ShadowPositionBook`;
- `SignalRouter`;
- executorów;
- mutowalnych kolekcji runtime.

Czas monotoniczny jest przeliczany przed materializacją na jawne `age_ms` i `elapsed_ms`.

### 2.2 Ochrona przed stale apply

Każdy snapshot posiada `PositionSnapshotGuard`:

- `position_id`;
- `position_epoch`;
- `state_revision`;
- `remaining_token_amount_raw`;
- latest sample slot;
- latest sample timestamp.

Jedynymi metodami modyfikującymi lifecycle po decyzji są:

```rust
begin_exit_proposal(snapshot_guard, candidate)
apply_shadow_quote_outcome(action_handle, quote_outcome)
terminate_shadow_proposal(action_handle, terminal_outcome)
```

Każda metoda:

- ponownie odnajduje pozycję pod krótkim write lockiem;
- sprawdza `position_id`, epoch, revision, action ID i remaining quantity;
- odrzuca stale/duplicate apply typed błędem;
- nigdy nie wykonuje częściowej mutacji;
- nigdy nie wykonuje `.await` pod lockiem.

Typed błędy apply:

- `PositionNotFound`;
- `EpochMismatch`;
- `StaleRevision`;
- `QuantityMismatch`;
- `ActionMismatch`;
- `ConcurrentActionPending`;
- `AlreadyTerminal`.

`StaleRevision` nie zamyka pozycji i nie emituje fill. Ocena zostaje ponowiona na kolejnym ticku.

### 2.3 Pure policy

Powstaje mały moduł `exit_policy_v1.rs`, bez I/O i zależności od runtime’u.

Główne interfejsy:

```rust
ExitPolicyV1::evaluate_prequote(
    snapshot: &PostBuyDecisionSnapshot,
    config: &EffectiveExitPolicyV1Config,
) -> PreQuoteDecision

ExitPolicyV1::finalize_with_quote(
    snapshot: &PostBuyDecisionSnapshot,
    candidate: &ExitCandidate,
    quote: &ExecutableExitQuote,
    config: &EffectiveExitPolicyV1Config,
) -> FinalPolicyDecision
```

`PreQuoteDecision`:

- `Hold`;
- `UnknownEvidence { reason }`;
- `QuoteRequired { candidate }`.

`FinalPolicyDecision`:

- `Hold`;
- `Exit { intent }`;
- `UnknownEvidence { reason }`.

`ExitCandidateReason`:

- `StopLoss`;
- `TakeProfit`;
- `Inactivity`;
- `AbsoluteMaxHold`;
- `CrashGuard`.

`UnknownEvidence` przed powstaniem exit condition jest diagnostyczne i nie zamyka pozycji. Wyjątki:

- gdy inactivity lub max-hold są już osiągnięte, brak mark price nie blokuje utworzenia pending proposal, ponieważ przesłanka czasowa jest znana;
- niepoprawne immutable entry identity, entry price lub remaining quantity są traktowane jako nierecoverable candidate data failure, a nie zwykły `Hold`.

### 2.4 Pending proposal i idempotencja

Pierwszy trigger atomowo tworzy:

```text
PendingExitProposal {
    action_id,
    position_id,
    position_epoch,
    reason,
    triggered_at_ms,
    recovery_deadline_ms,
    expected_remaining_quantity,
    source_snapshot_id
}
```

`action_id` jest deterministycznie wyprowadzany z:

```text
position_id + position_epoch + next_exit_action_seq
```

Po utworzeniu proposal:

- proposal jest sticky;
- chwilowe cofnięcie mark price nie anuluje już rozpoczętej próby wyjścia;
- kolejny tick nie tworzy drugiego action ID;
- nie może istnieć więcej niż jedno pending sell dla tej samej epoki;
- każdy retry korzysta z najświeższego dostępnego snapshotu;
- quantity nie jest zmniejszana przed `SimulatedFilled`;
- duplicate lub spóźniony outcome dla starego action ID jest ignorowany typed błędem.

### 2.5 Leniwy executable quote

Quote jest wyliczany dopiero wtedy, gdy:

- `evaluate_prequote()` zwróci `QuoteRequired`;
- istnieje wcześniejszy `PendingExitProposal`;
- CrashGuard w trybie obserwacyjnym spełni tani warunek price-path i wymaga potwierdzenia quote’em.

Quote nie jest liczony dla zwykłego `Hold`.

Źródłem pozostaje obecny `PriceTruthResolver`:

- bez RPC;
- bez `.await`;
- na snapshotach z `AccountStateCore`;
- dla pełnej `remaining_token_amount_raw`;
- z bieżącymi reserves;
- z bonding-curve impact.

W obrębie jednego ticku stosowana jest lokalna, nietrwała lazy cell indeksowana przez:

```text
position_id + epoch + state_revision + remaining_qty + slot + timestamp
```

Dzięki temu:

- authoritative policy i observe-only CrashGuard mogą użyć tego samego quote;
- quote nie jest cache’owany między tickami;
- nie ma ryzyka użycia starego curve state;
- nie powstają pre-signed bullets ani odświeżany jedynie blockhash.

Pierwsza wersja quote’u jest jawnie klasyfikowana jako:

```text
position_sized_curve_executable_gross_costs_unmodeled
```

Obecne `estimated_costs_sol = 0` może pozostać dla kompatybilności starych rekordów, ale nowe pola muszą mówić:

```text
net_pnl_authoritative = false
execution_cost_coverage = "unmodeled"
```

Nie wolno przedstawiać tego outcome jako wiarygodnego net PnL.

### 2.6 Semantyka Unknown

| Lane | Znaczenie | Zachowanie |
|---|---|---|
| Shadow | Nie udało się rozstrzygnąć symulowanego full exit | Retry co 500 ms maksymalnie przez 5 s; potem kanoniczny `BLOCKED_BY_DATA`/`NO_FILL`/`FAILED`, bez fill i PnL; usunięcie syntetycznej pozycji i zwolnienie wyłącznie shadow slotu |
| Live | SELL mógł zostać wysłany, ale jego on-chain outcome nie jest jednoznaczny | `ReconciliationPending`; brak kolejnego SELL, brak zmiany quantity, brak `record_closed`, brak zwolnienia slotu; registry pozostaje Open |

Live `Uncertain` nie może być nadal mapowany do retryable `ExitConfirmFailed`, ponieważ obecne zachowanie może zbudować i wysłać drugi SELL po nierozstrzygniętym pierwszym SELL-u.

### 2.7 Lifecycle shadow

Stan aktywnego shadow lifecycle:

```text
Open
  → QuotePending
      → SimulatedFilled
          → Closed
      → BlockedByData / NoFill / Failed
          → TerminalBlocked
```

Tylko `SimulatedFilled` prowadzi do:

- zmniejszenia remaining quantity;
- `ExitFilled`;
- `PositionClosed`;
- finalnego PnL;
- `TerminalReasonV2::Target`, `Stop` albo `Timeout`.

`BlockedByData`, `NoFill` i `Failed` prowadzą do terminalnego shadow outcome, ale nie do ekonomicznego close.

---

## 3. PR 1 — Position Manager Lite Core i poprawny lifecycle

### 3.1 Pure policy z pełną parity

Należy zastąpić aktywną implementację `run_shadow_simple_threshold_tick()` adapterem do jednego `ExitPolicyV1`.

PR 1 obsługuje wyłącznie dotychczasową politykę:

1. typed data integrity;
2. stop-loss −50%;
3. take-profit +50%;
4. inactivity 30 s;
5. hold.

Kolejność stop-loss → take-profit → inactivity pozostaje identyczna z baseline.

Na PR 1:

- max-hold jest nieaktywny;
- CrashGuard jest nieaktywny;
- Guardian signals nie są wejściem policy;
- AEM nie jest wejściem policy;
- Revolver nie jest wejściem policy;
- nie ma partial exits;
- nie ma trailing stop.

Stary evaluator nie może pozostać równoległym produkcyjnym evaluatorem. Po cutoverze:

- `run_shadow_simple_threshold_tick()` zostaje usunięty albo staje się cienkim adapterem bez własnych reguł;
- `determine_shadow_simple_exit_trigger()` nie może pozostać drugim policy ownerem;
- wszystkie aktywne i probe shadow positions używają tego samego pure evaluatora.

### 3.2 Lazy quote i guarded apply

Przebieg udanego exit:

1. materializacja snapshotu pod krótkim read lockiem;
2. pure prequote evaluation bez locka;
3. guarded utworzenie pending proposal;
4. position-sized quote bez locka;
5. pure finalize;
6. guarded apply;
7. dopiero potem emisja `ExitSubmitted`;
8. `ExitFilled { status: Filled }`;
9. `PositionClosed`;
10. canonical `ShadowTerminalTruthV2`;
11. terminal notification do launchera;
12. zwolnienie shadow slotu.

Shadow fill nie może być już oznaczany `Confirmed`. `Confirmed` pozostaje semantyką rzeczywistego on-chain confirmation; symulacja używa `Filled`.

### 3.3 Usunięcie fałszywego close bez truth

Należy całkowicie usunąć aktywne użycie `force_close_shadow_without_exit_truth()`.

Brak snapshotu, stale snapshot, brak normalizacji, invalid reserves albo semantic violation po powstaniu exit proposal:

- tworzy/deduplikuje `ExitBlocked`;
- zachowuje pending action ID;
- ponawia lazy quote przez maksymalnie 5 s;
- nie zmienia quantity;
- nie emituje `ExitFilled`;
- nie emituje `PositionClosed`;
- nie emituje PnL.

Po 5 s powstaje terminalny shadow outcome z zachowaniem najbardziej precyzyjnego powodu:

- `BlockedByData`;
- `NoFill`;
- `Failed`.

Obecny generic `unregister_position()` należy zastąpić trzema semantycznymi operacjami:

- resolved close;
- unresolved shadow termination;
- administracyjne usunięcie przy shutdown/test cleanup bez fałszywego lifecycle eventu.

Żadna zewnętrzna utrata wpisu w `ShadowPositionBook` nie może wywoływać resolved close.

### 3.4 Canonical terminal truth i eventy

Istniejący `ShadowTerminalTruthV2` pozostaje kanonicznym SSOT.

Dla unresolved shadow:

- terminal reason to `BlockedByData`, `NoFill` albo `Failed`;
- `final_pnl_mark_bps = None`;
- `final_pnl_executable_bps = None`;
- `linked_exit_fill = None`;
- derived `shadow_lifecycle_v2.lifecycle_event_type = TerminalBlocked`.

Do ogólnego event schema należy dodać operacyjną projekcję:

```rust
EventKind::ShadowPositionUnresolved(ShadowPositionUnresolvedPayload)
```

Payload:

- typed unresolved reason;
- action ID;
- policy ID/version/config hash;
- remaining raw quantity;
- recovery elapsed;
- ostatni price-truth status i provenance;
- source snapshot ID.

Payload nie posiada PnL ani fill price.

`EventValidator` ma uznawać trajectory za terminalną, jeżeli ma dokładnie jedno z:

- `PositionClosed`;
- `ShadowPositionUnresolved`.

Dodatkowe reguły:

- unresolved jest dozwolone tylko dla `Lane::Shadow`;
- close i unresolved są wzajemnie wykluczające;
- unresolved wymaga wcześniejszego `PositionOpened`;
- unresolved nie trafia do close/PnL denominators;
- stare JSONL nadal się deserializują;
- successful legacy lifecycle rows zachowują istniejące pola.

### 3.5 Explicit terminal notification zamiast pollingu

`PostBuyRuntime` nie powinien wnioskować wyniku wyłącznie z `get_position_health() == None`.

Aktywna rejestracja shadow zwraca prywatny terminal receiver powiązany dokładnie z `(position_id, epoch)`.

Terminalne wyniki:

```rust
ShadowTerminalDisposition::SimulatedClosed {
    action_id,
    reason,
}

ShadowTerminalDisposition::SimulationBlocked {
    action_id,
    reason,
}
```

Launcher:

- zwalnia shadow slot dla obu terminalnych wyników;
- nie interpretuje `SimulationBlocked` jako close;
- loguje osobne metrics dla closed i blocked;
- przy utracie terminal channel zwalnia shadow slot z typed `terminal_channel_dropped`, ponieważ shadow nie reprezentuje on-chain exposure;
- nie stosuje tej reguły do live.

### 3.6 Live Unknown bez aktywowania live

Dormant live lane pozostaje osobną ścieżką wykonawczą, ale otrzymuje poprawną taksonomię:

- dodać `LiveExitStatus::ExitConfirmationUnknown` albo równoważne `ReconciliationPending`;
- `SenderSellAttemptConfirmation::Uncertain` mapuje się do tego statusu;
- status nie jest retryable;
- nie buduje i nie wysyła następnego SELL;
- zachowuje exit signature;
- nie zamyka registry;
- nie zmniejsza visible/remaining quantity;
- nie zwalnia slotu;
- kończy bieżący task jako wymagający reconciliation.

Nie należy w tym PR aktywować live ani zmieniać +58%/−46%.

### 3.7 Guardian, AEM i virtual Revolver

Aktywna ścieżka shadow ma używać Guardianów wyłącznie obserwacyjnie:

- LIGMA/WHF/TCF/PANIC mogą nadal liczyć i logować evidence;
- każdy ich wynik jest oznaczony `consumed_by_policy=false`;
- aktywny `SignalRouter` działa w trybie observation-only;
- nie wykonuje `TightenStop`;
- nie wykonuje `PanicSell`;
- nie mutuje virtual magazine;
- zniknięcie lub niespójność `ShadowPositionBook` nie może zamknąć `MonitoredPosition`;
- mirror może zostać naprawiony/reseedowany albo pominięty, ale pozostaje bez authority.

AEM:

- nie zostaje podłączony;
- `[post_buy_guardian.aem].enabled` w aktywnym TOML zostaje ustawione na `false`;
- paper/test compatibility pozostaje;
- learned regimes i auto-rollout pozostają poza aktywną ścieżką.

Revolver worker i pre-signed bullets pozostają niewykorzystywane.

### 3.8 Pełne i jednoznaczne config wiring

`PostBuyRuntimeConfig` nie powinien kopiować kilku wybranych pól Guardiana i reszty zastępować defaultami.

Launcher przekazuje pełny `PostBuyGuardianConfig`.

Runtime-owned overlay może zmienić wyłącznie:

- `max_monitored_positions` na realne `max_concurrent_positions`;
- ścieżki runtime artefaktów;
- lane/runtime mode.

TP/SL/inactivity pozostają w dotychczasowych polach TOML, aby nie wykonywać niepotrzebnej migracji configu.

Powstaje immutable `EffectiveExitPolicyV1Config`, walidowany przed uruchomieniem monitora. Hash:

- serializacja jawnego effective config struct;
- `serde_json`;
- BLAKE3, analogicznie do istniejącego V3 config hash.

Startup blokuje:

- niefinitywne lub ujemne TP;
- stop-loss poza `0..=100%`;
- zerowy inactivity timeout;
- zerowy quote recovery;
- shadow mode z wyłączonym Guardianem;
- próbę ustawienia AEM/Revolver/Guardian signal authority w aktywnym profilu.

Emitowany jest jeden strukturalny rekord startowy z:

- policy ID/version/hash;
- lane;
- TP/SL/inactivity;
- quote recovery 5 s;
- Guardian authority `observation_only`;
- AEM `disabled`;
- Revolver `disabled`;
- live `disabled`.

### 3.9 Bounded direct post-buy handoff

`mpsc::unbounded_channel()` zostaje zastąpione bounded `mpsc::channel()`.

Pojemność:

```text
clamp(max_concurrent_positions × 4, 8, 256)
```

Producent używa `try_send`, bez `.await` na hot path.

`TrySendError` jest rozróżniony:

- `Full`;
- `Closed`.

Semantyka:

- shadow handoff failure/rejection powoduje zwolnienie shadow reservation przez istniejącą RAII lease;
- confirmed live handoff failure zachowuje slot fail-closed;
- brak fallbacku z direct queue do niejawnej innej ścieżki;
- metrics zawierają lane, transport i dokładny reason.

### 3.10 Minimalny kontrakt outcome pod przyszły Type-5

Dla resolved close logowane są:

- policy ID/version/config hash;
- snapshot ID;
- action ID i epoch;
- entry i exit raw quantity;
- mark return;
- position-sized executable gross return;
- MFE i MAE mark z już utrzymywanej bounded timeline;
- peak drawdown;
- hold age;
- inactivity age;
- quote source/slot/timestamp/freshness;
- reserve inputs i impact;
- execution-cost coverage;
- terminal reason;
- terminal disposition.

Dla unresolved:

- te same identity/provenance fields;
- brak fill i PnL;
- typed blocker;
- recovery duration;
- remaining quantity.

Nie powstaje jeszcze Type-5 model ani expected-EV predictor.

### 3.11 Warunki akceptacji PR 1

PR 1 jest kompletny, gdy:

- istnieje jeden pure shadow exit evaluator;
- +50%, −50% i 30 s mają dokładną parity z baseline na clean fixtures;
- quote nie jest liczony dla `Hold`;
- quote jest liczony najwyżej raz na pozycję/tick;
- stale apply nie mutuje pozycji;
- tylko resolved quote może wywołać `ExitFilled` i `PositionClosed`;
- shadow fill ma status `Filled`, nie `Confirmed`;
- brak/stale/invalid truth nie tworzy fałszywego close;
- po 5 s unresolved kończy się `TerminalBlocked`, bez PnL;
- canonical Shadow V2 i operacyjny event zgadzają się co do disposition;
- slot jest zwalniany na podstawie typed shadow terminal outcome;
- live uncertainty nie uruchamia drugiego SELL i nie zwalnia slotu;
- Guardian signals nie zmieniają lifecycle;
- virtual magazine nie jest position authority;
- direct handoff nie jest unbounded;
- pełny Guardian config dociera do runtime;
- live pozostaje nieaktywne;
- prebuy Decision Plane nie zmienia żadnego BUY/REJECT/TIMEOUT.

Rollback PR 1 jest pełnym revertem PR. Nie należy utrzymywać starego i nowego evaluatora równolegle.

---

## 4. PR 2 — Absolute max-hold i CrashGuard V1

### 4.1 Nowy config

Do `PostBuyGuardianConfig` zostaje dodany backward-compatible `ExitPolicyV1Config` z `#[serde(default)]`.

Brak sekcji w starym configu oznacza:

- absolute max-hold disabled;
- CrashGuard disabled;
- żadnej ukrytej zmiany starego profilu.

Aktywny TOML jawnie ustawia:

```text
quote_recovery_ms = 5000

absolute_max_hold_enabled = true
absolute_max_hold_ms = 120000

crash_guard_mode = "observe_only"
crash_window_ms = 1500
crash_min_short_window_drop_pct = 25.0
crash_min_peak_drawdown_pct = 30.0
crash_min_distinct_slots = 2
crash_max_sample_age_ms = 1500
crash_max_executable_return_pct = -20.0
```

Dozwolone `CrashGuardMode`:

- `disabled`;
- `observe_only`;
- `authoritative_shadow`.

Nie istnieje automatyczna promocja.

`authoritative_shadow` jest odrzucane przy startupie, jeżeli:

- `execution_mode != shadow`;
- `entry_mode != shadow_only`;
- live sender/dispatch jest aktywny;
- wymagane quote/evidence logging jest wyłączone.

### 4.2 Docelowa kolejność policy

Po PR 2 pełna kolejność logiczna to:

1. immutable/candidate data integrity;
2. istniejący sticky pending proposal;
3. CrashGuard;
4. hard stop −50%;
5. take-profit +50%;
6. inactivity 30 s;
7. absolute max-hold 120 s;
8. hold.

W aktywnym profilu:

- CrashGuard jest obliczany w tej pozycji logicznej, ale `observe_only` nie preemptuje authoritative result;
- max-hold jest authoritative wyłącznie dla shadow;
- stop-loss, TP i inactivity zachowują obecne progi.

Jeżeli jednocześnie spełnione są inactivity i max-hold, reason pozostaje `Inactivity`, zgodnie z kolejnością.

### 4.3 Absolute max-hold

Max-hold:

- używa absolute age od `entry_unix_ms`;
- nie zależy od activity heartbeat;
- nie może zostać zresetowany przez transakcje;
- przy 119 999 ms nie działa;
- przy 120 000 ms tworzy full-exit proposal;
- korzysta z tego samego lazy quote i 5-sekundowego recovery;
- nie wprowadza partial exit;
- jest wyłączalny jednym polem configu.

Każdy max-hold exit zapisuje:

- `would_hold_under_legacy_inactivity_policy=true/false`;
- absolute age;
- inactivity age;
- capacity occupancy age;
- policy version/config hash.

### 4.4 CrashGuard evidence

CrashGuard nie używa composite PANIC score jako hard triggera.

Tani prequote candidate wymaga łącznie:

- co najmniej dwóch kolejnych poprawnych kanonicznych price samples;
- sample pochodzą z różnych slotów;
- sample są monotonicznie malejące;
- newest sample nie jest starszy niż 1500 ms;
- oldest-to-newest spadek w oknie maksymalnie 1500 ms wynosi co najmniej 25%;
- drawdown od kanonicznego peak od entry wynosi co najmniej 30%;
- slot/timestamp ordering nie jest odwrócony ani sprzeczny.

Nie ma one-sample catastrophic override w V1.

Po spełnieniu taniego warunku następuje jedno leniwe position-sized quote.

`CrashConfirmed` wymaga:

- poprawnego executable quote;
- dodatniego output;
- pełnej pozostałej quantity;
- quote z tego samego albo nowszego, nadal świeżego evidence revision;
- executable gross return względem entry nie większego niż −20%.

W trybie `observe_only` zapisuje się:

- `CrashNotTriggered`;
- `CrashCandidate`;
- `CrashConfirmed`;
- `CrashRejectedByQuote`;
- `CrashBlockedByData`.

Emisja odbywa się tylko przy candidate/state transition, nie na każdym zwykłym ticku.

Każdy rekord zawiera `consumed_by_policy=false`.

Zmiana na `authoritative_shadow` w przyszłości nie wymaga nowego kodu: ten sam `CrashConfirmed` tworzy `EmergencyExit` i preemptuje stop-loss/TP. W obecnym planie config pozostaje `observe_only`.

### 4.5 Peak i compact history

Peak musi być aktualizowany niezależnie od AEM, wyłącznie przez przyjęcie poprawnego kanonicznego snapshotu.

Crash snapshot nie kopiuje całej timeline. Materializuje wyłącznie:

- peak;
- latest sample;
- poprzedni właściwy distinct-slot sample;
- oldest sample mieszczący się w crash window;
- return;
- drawdown;
- freshness;
- slot/timestamp provenance;
- licznik poprawnych distinct slots.

Istniejący bounded timeline pozostaje ograniczony. Nie powstaje unbounded history.

### 4.6 Baseline versus candidate

Dla każdego CrashGuard candidate logowane są dwie wartości:

- `authoritative_decision`;
- `crash_guard_candidate_decision`.

W `observe_only`:

- candidate nie tworzy pending sell;
- nie zmienia quantity;
- nie usuwa pozycji;
- nie zwalnia slotu;
- nie zmienia close reason;
- nie zmienia canonical terminal truth.

Jeżeli w tym samym ticku authoritative TP/SL/inactivity/max-hold również potrzebuje quote’u, oba wyniki wykorzystują tę samą lokalną lazy quote cell.

### 4.7 Warunki akceptacji PR 2

PR 2 jest kompletny, gdy:

- max-hold działa dokładnie od 120 000 ms;
- activity heartbeat nie resetuje max-hold;
- inactivity ma pierwszeństwo przed max-hold;
- CrashGuard nie przechodzi przy tym samym slocie;
- CrashGuard nie przechodzi przy stale, reversed lub non-descending evidence;
- każde z kryteriów crash ma osobny test negatywny;
- candidate bez executable quote nie staje się confirmed;
- CrashGuard w `observe_only` nigdy nie zmienia lifecycle;
- aktywny max-hold używa tego samego quote/apply/outcome contractu co TP/SL/inactivity;
- konfiguracja może wyłączyć max-hold bez kodowego rollbacku;
- próba aktywacji CrashGuard poza czystym shadow profile blokuje startup;
- live nadal używa +58%/−46% i pozostaje dormant;
- żaden Guardian/AEM/Revolver signal nie staje się niejawnie authoritative.

---

## 5. Testy i kontrakt braku regresji

### 5.1 Pure policy

Obowiązkowe golden tests:

- identyczny snapshot/config → identyczny result;
- dokładna granica −50%;
- wartość tuż powyżej −50%;
- dokładna granica +50%;
- wartość tuż poniżej +50%;
- dokładnie 30 000 ms inactivity;
- 29 999 ms inactivity;
- stop-loss przed TP;
- TP przed inactivity;
- inactivity przed max-hold;
- brak mark data przed time condition → `UnknownEvidence`, bez terminalizacji;
- time condition z brakiem mark data → pending quote recovery;
- policy source guard: brak locków, RPC, `Instant`, `AccountStateCore`, executora i mutable runtime imports.

### 5.2 Snapshot/apply

- private fields nie są dostępne poza modułem;
- stale revision odrzuca apply;
- epoch mismatch odrzuca apply;
- quantity mismatch odrzuca apply;
- duplicate action outcome jest idempotentny;
- spóźniony outcome starego action ID nie dotyka nowej epoki;
- jeden pending proposal blokuje drugi sell;
- brak `.await` pod positions lock;
- snapshot nie zmienia się po późniejszej mutacji pozycji.

### 5.3 Lazy quote

- `Hold` → zero wywołań resolvera;
- `UnknownEvidence` bez exit condition → zero wywołań;
- pierwszy proposal → dokładnie jedno wywołanie;
- pending retry → najwyżej jedno wywołanie na tick;
- baseline i CrashGuard candidate na tym samym snapshotcie → jedno wspólne wywołanie;
- quote z poprzedniego ticku nie jest reużywany;
- quote zawsze używa pełnej remaining raw quantity.

### 5.4 Shadow outcomes

- resolved stop-loss → `ExitSubmitted`, `ExitFilled(Filled)`, `PositionClosed`;
- resolved TP → ten sam poprawny lifecycle;
- resolved inactivity → ten sam poprawny lifecycle;
- stale quote przez mniej niż 5 s → pozycja nadal pending;
- quote odzyskany przed 5 s → jeden fill i jeden close;
- quote nierozstrzygnięty przez 5 s → `ShadowPositionUnresolved`, bez fill/close/PnL;
- zero executable output → `NoFill`, nie `PositionClosed`;
- semantic violation → `BlockedByData`;
- terminal canonical Shadow V2 reason odpowiada operational event;
- derived lifecycle używa `TerminalBlocked`;
- slot jest zwalniany po shadow closed i shadow blocked;
- dropped terminal channel ma osobny reason i nie dotyka live semantics.

Należy odwrócić obecny test, który oczekuje `PositionClosed` po stale time-stop. Nowa asercja ma wymagać unresolved/terminal blocked i braku fill/PnL.

### 5.5 Live Unknown

- `Confirmed` zamyka registry i zwalnia slot;
- `Failed` nie zwalnia slotu;
- `Uncertain/Unknown` nie jest retryable;
- po Unknown nie powstaje drugi SELL;
- exit signature zostaje zachowana;
- quantity i registry Open pozostają;
- slot pozostaje zajęty;
- live config i dispatch pozostają wyłączone.

### 5.6 Config i handoff

- stary TOML bez nowych pól ładuje się z safe defaults;
- aktywny TOML daje +50/−50/30 s, 5 s recovery, max-hold 120 s i CrashGuard observe-only;
- full Guardian thresholds z TOML docierają bez default-shadowing;
- AEM jest raportowany jako disabled/unwired;
- kolejka ma wyliczoną bounded capacity;
- queue full shadow nie pozostawia orphan slotu;
- queue full confirmed live zachowuje slot fail-closed;
- queue closed ma odrębny reason;
- direct producer nie wykonuje await.

### 5.7 Guardian i mirror isolation

- dowolna sekwencja LIGMA/WHF/TCF/PANIC nie zmienia policy result;
- `PanicSell` z SignalRoutera nie zamyka aktywnej pozycji;
- zniknięcie pozycji z `ShadowPositionBook` nie usuwa `MonitoredPosition`;
- mirror może zostać naprawiony bez zmiany action ID/quantity;
- AEM enabled w aktywnym profilu jest odrzucone albo raportowane jako disabled zgodnie z effective config;
- paper AEM tests pozostają zielone.

### 5.8 Minimalne uruchomienia

Po każdym PR:

```bash
cargo test -p trigger entry_price_extractor
cargo test -p ghost-brain guardian::post_buy
cargo test -p ghost-brain events::validator
cargo test -p ghost-launcher components::post_buy_runtime
cargo test -p ghost-launcher --test post_buy_runtime_integration
cargo clippy -p trigger -p ghost-brain -p ghost-launcher --all-targets -- -D warnings
```

Dodatkowo należy uruchomić istniejące changed-crate, logger/replay i shadow lifecycle suites używane przez repozytoryjne CI.

Merge jest blokowany przez:

- jakąkolwiek parity difference PR 1 na clean TP/SL/inactivity fixtures;
- duplicate terminal;
- unresolved z PnL;
- live Unknown zwalniające slot;
- stale apply zmieniające quantity;
- aktywne Guardian/AEM/Revolver authority;
- niezielone pełne wymagane CI.

---

## 6. Świadomie poza zakresem

Ten plan nie implementuje:

- nowego `PositionStore`;
- pełnego AEM;
- learned regimes;
- automatic rollout;
- partial exits;
- ladder exits;
- normalnego trailing stop;
- WaitReclaim;
- defensive partial;
- Revolver worker;
- pre-signed SELL bullets;
- dynamic tip/fee model dla shadow;
- uruchomienia live execution;
- pełnego durable live recovery workera;
- atomicznego live registry/capacity persistence;
- Type-5;
- expected-EV predictora;
- zmian Gatekeeper V2/V2.5/V3;
- zmian BUY/REJECT/TIMEOUT;
- zmian progów +50/−50/30 s albo dormant live +58/−46.

Po PR 2 następnym krokiem jest wspólny shadow burn-in:

- Position Manager produkuje stabilne, poprawnie sklasyfikowane outcome;
- Type-5 Lite może zostać zbudowany na tych outcome jako observe-only;
- CrashGuard pozostaje observe-only do osobnej decyzji o config promotion;
- nie ma automatycznego przejścia na authority.

Dokumentacja jest ograniczona do:

- jednego zaakceptowanego planu `.md`;
- jednego wymaganego ADR-8D na PR;
- aktualizacji istniejących komentarzy/schema docs bez osobnych audytów i manifestów.

---

## 7. Definition of Done

- [ ] `MonitoredPosition` pozostaje jedynym kanonicznym stanem aktywnej pozycji shadow.
- [ ] Wszystkie jego pola są prywatne.
- [ ] Policy otrzymuje wyłącznie immutable snapshot.
- [ ] Mutacja odbywa się wyłącznie przez guarded begin/apply.
- [ ] Istnieje jeden pure `ExitPolicyV1`.
- [ ] Stary simple evaluator nie ma równoległego authority.
- [ ] Executable quote jest position-sized i lazy.
- [ ] `Hold` nie uruchamia quote.
- [ ] Nie ma RPC ani await w shadow policy/quote hot path.
- [ ] +50%, −50% i 30 s zachowują baseline parity.
- [ ] Pending proposal jest sticky i idempotentny.
- [ ] Stale outcome nie może zostać zastosowany.
- [ ] Shadow fill używa statusu `Filled`, nie `Confirmed`.
- [ ] Brak truth nigdy nie emituje `PositionClosed`.
- [ ] Shadow unresolved po 5 s nie ma fill ani PnL.
- [ ] Shadow unresolved zwalnia shadow slot.
- [ ] Live Unknown nie zamyka pozycji, nie retryuje SELL i nie zwalnia slotu.
- [ ] Canonical `ShadowTerminalTruthV2` pozostaje terminalnym SSOT.
- [ ] Derived unresolved lifecycle ma `TerminalBlocked`.
- [ ] General event validator rozróżnia closed od unresolved.
- [ ] Direct handoff jest bounded.
- [ ] Guardian signals są observation-only.
- [ ] ShadowPositionBook nie ma authority nad canonical lifecycle.
- [ ] AEM jest disabled w aktywnym profilu.
- [ ] Revolver bullets nie są używane.
- [ ] Max-hold działa od 120 s niezależnie od heartbeat.
- [ ] CrashGuard działa jako observe-only z dwiema distinct-slot próbkami i lazy quote.
- [ ] Nie istnieje one-sample panic override.
- [ ] Outcome zawiera policy/config/action/snapshot provenance.
- [ ] Gross executable PnL nie jest nazywany wiarygodnym net PnL.
- [ ] Live pozostaje disabled.
- [ ] Prebuy Decision Plane pozostaje bez zmian.
- [ ] Pełne wymagane CI jest zielone.

---

## 8. Założenia i routing

```yaml
task_classification: cross-cutting post-buy architecture and lifecycle safety
primary_specialist: Ghost Runtime Coordinator
supporting_specialists:
  - Solana Execution Path Engineer
  - Decision Logging Replay Analyst
  - Config Rollout Safety Reviewer
  - Rust Runtime Engineer
skills_used:
  - ghost-execution
  - trading-systems
  - rust-master
  - solana-pumpfun-architect
  - statistical-research-engine
runtime_area_touched:
  - active shadow post-buy lifecycle
  - dormant live confirmation taxonomy
  - Guardian observation wiring
  - post-buy handoff backpressure
  - lifecycle events and Shadow V2 terminal truth
contracts_at_risk:
  - canonical position ownership
  - shadow/live separation
  - submit versus confirmation
  - unknown outcome semantics
  - replay and terminal truth
  - capacity release
  - config backward compatibility
active_or_legacy_path: active shadow plus safety-only correction of dormant live
risk_level: medium-high
recommended_action: implement in two PRs, with exact parity gate after PR 1 and hybrid rollout in PR 2
```

```yaml
delegation_trace:
  task_classification: "cross-cutting post-buy architecture and lifecycle safety"
  routing_performed: true
  primary_specialist: "Ghost Runtime Coordinator"
  supporting_specialists_considered:
    - "Solana Execution Path Engineer"
    - "Decision Logging Replay Analyst"
    - "Config Rollout Safety Reviewer"
    - "SSOT Feature Materialization Guardian"
    - "Gatekeeper Policy Auditor"
  specialist_docs_loaded:
    - "docs/agents/ghost-runtime-coordinator.md"
    - "docs/agents/solana-execution-path-engineer.md"
    - "docs/agents/decision-logging-replay-analyst.md"
    - "docs/agents/config-rollout-safety-reviewer.md"
  specialist_docs_not_loaded:
    - name: "SSOT Feature Materialization Guardian"
      reason: "Prebuy MaterializedFeatureSet and feature authority are explicitly outside this implementation."
    - name: "Gatekeeper Policy Auditor"
      reason: "Plan does not change Gatekeeper V2, V2.5, V3, BUY, REJECT or TIMEOUT."
    - name: "Oracle Session Runtime Engineer"
      reason: "Oracle observation-session lifecycle is not modified; only bounded post-buy handoff is touched."
    - name: "Seer Ingest Event Integrity Specialist"
      reason: "Ingest parsing, ordering and transaction admission remain unchanged."
  skills_used:
    - "ghost-execution"
    - "trading-systems"
    - "rust-master"
    - "solana-pumpfun-architect"
    - "statistical-research-engine"
  fast_path_used: false
  contracts_checked:
    - "single canonical shadow position owner"
    - "private Rust state boundary"
    - "immutable snapshot and guarded apply"
    - "no lock across await"
    - "bounded concurrency and backpressure"
    - "shadow simulation is not live confirmation"
    - "submit is not confirmation"
    - "unknown is not success"
    - "typed terminal reason and replay truth"
    - "shadow/live capacity release semantics"
    - "config serde backwa
