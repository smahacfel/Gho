# ADR-8D: Position Manager Lite V1 — jeden shadow exit authority i poprawny lifecycle

Status: `IMPLEMENTED / LOCAL VALIDATION COMPLETE / CI PENDING`

Typ: ADR-8D / aktywny post-buy runtime / shadow lifecycle / dormant live safety

Data: 2026-07-15

Repo: `smahacfel/Gho`

Branch: `agent/position-manager-lite-pr1-20260715`

Base SHA: `53382696eb06affbd309ca4d050f030d31a561b0`

Plan wykonawczy:
`PLANS/DO_REALIZACJI/POSITION_MANAGER_LITE_V1.md`, rozdział 3 — PR 1.

Poziom ryzyka: `HIGH` — zmiana obejmuje aktywny lifecycle shadow po BUY,
terminalizację pozycji, zwalnianie capacity, schemat eventów oraz dormant live
confirmation taxonomy. Nie zmienia jednak prebuy Decision Plane, nie aktywuje
live i nie zmienia ekonomicznych progów aktywnego shadow.

## 1. Problem

Przed tym cutoverem aktywny post-buy shadow łączył w `MonitoringEngine` kilka
konkurencyjnych semantyk:

1. osobny evaluator `run_shadow_simple_threshold_tick()` wykonywał pełny exit
   przy stop-loss, take-profit albo inactivity;
2. Guardian signals i virtual Revolver mutowały drugi model pozycji, lecz
   mutacje były pomijane przez aktywny simple-threshold path;
3. brak lub nieświeża price truth przy inactivity mógł prowadzić do
   `force_close_shadow_without_exit_truth()`, synthetic close i zwolnienia
   capacity bez realizowalnego fill;
4. shadow fill używał statusu `Confirmed`, mimo że nie był potwierdzeniem
   on-chain;
5. launcher zwalniał shadow slot na podstawie pollingu zniknięcia pozycji,
   zamiast typed terminal outcome;
6. direct post-buy handoff był kolejką bez limitu;
7. `PostBuyRuntimeConfig` kopiował tylko część `PostBuyGuardianConfig`, a
   pozostałe pola zastępował defaultami;
8. live `Uncertain` był spłaszczany do confirmation failure, bez jawnego
   statusu wymagającego reconciliation.

Skutkiem był działający, ale niespójny model lifecycle: brak jednoznacznego
authority, możliwość fałszywego close bez truth oraz niewystarczający kontrakt
outcome pod późniejszą kalibrację Type-5 i strategii exit.

## 2. Decyzja architektoniczna

PR 1 wprowadza jeden minimalny Position Manager Lite bez aktywowania nowych
strategii ekonomicznych:

```text
MonitoredPosition (private canonical state)
  -> immutable PostBuyDecisionSnapshot
  -> pure ExitPolicyV1::evaluate_prequote()
  -> Hold / typed Unknown / QuoteRequired
  -> sticky guarded exit proposal
  -> lazy position-sized executable quote poza lockiem
  -> pure ExitPolicyV1::finalize_with_quote()
  -> guarded apply
  -> typed shadow terminal outcome
  -> canonical lifecycle + ShadowTerminalTruthV2
  -> explicit terminal notification
  -> capacity release
```

`PostBuyRuntime` pozostaje zewnętrznym orkiestratorem. `MonitoringEngine`
pozostaje ownerem aktywnego stanu shadow w PR 1, lecz jego stan ma prywatne
pola i może wejść do decyzji wyłącznie przez immutable snapshot, a wynik
wrócić przez sprawdzony apply interface.

Nie powstał drugi position store, drugi evaluator ani nowy framework
execution. Jest to bezpośredni cutover istniejącej aktywnej ścieżki.

## 3. Jedyna aktywna polityka PR 1

`ExitPolicyV1` jest pure: nie posiada I/O, zegara runtime, locków, senderów,
RPC ani dostępu do `MonitoringEngine`.

Kolejność reguł zachowuje baseline:

1. typed integralność snapshotu;
2. stop-loss `-50%`;
3. take-profit `+50%`;
4. market inactivity `30 s`;
5. hold.

Progi nadal pochodzą z aktywnego TOML:

- `target_threshold = 50.0`;
- `stoploss_threshold = 50.0`;
- `wait_for_timestop = 30000`.

Granice są inkluzywne zgodnie z dotychczasowym zachowaniem:

- `mark <= entry * (1 - stop_loss)` uruchamia stop-loss;
- `mark >= entry * (1 + take_profit)` uruchamia take-profit;
- `inactivity_age >= timeout` uruchamia inactivity.

PR 1 celowo nie aktywuje:

- absolute max-hold;
- CrashGuard;
- partial exits;
- trailing stop;
- AEM policy/regimes/learning;
- Guardian composite authority;
- Revolver bullets lub magazine authority.

Stary `run_shadow_simple_threshold_tick()`,
`determine_shadow_simple_exit_trigger()` i
`force_close_shadow_without_exit_truth()` nie mają aktywnego odpowiednika ani
równoległego produkcyjnego consumera.

## 4. Granica modułowa i guarded apply

`MonitoredPosition` pozostaje prywatnym typem modułu. Zewnętrzny kod nie może
bezpośrednio mutować:

- remaining quantity;
- position epoch;
- pending proposal;
- action sequence;
- terminal outcome;
- peak/timeline;
- state revision.

`PostBuyDecisionSnapshot` również ma prywatne pola. Snapshot zawiera między
innymi:

- `position_id`, `position_epoch` i `state_revision`;
- entry i remaining raw quantity;
- mark status, source, slot, timestamp i freshness;
- inactivity i absolute age;
- bounded MFE/MAE oraz peak/drawdown evidence;
- reserve inputs potrzebne do późniejszej attribution;
- effective policy config hash;
- deterministyczny snapshot ID.

Każda mutacja po ewaluacji sprawdza `PositionSnapshotGuard` albo
`ShadowExitActionHandle`. Apply odrzuca:

- inną pozycję;
- inną epokę;
- stale revision;
- zmianę remaining quantity;
- niezgodny action ID;
- konkurencyjną akcję;
- pozycję już terminalną.

Timeline refresh nie wpisuje już nowego snapshotu przed guarded apply. Jeden
punkt `remember_shadow_snapshot()` aktualizuje last snapshot, peak i revision,
dzięki czemu późniejszy evaluator nie pracuje na niezarejestrowanej zmianie
stanu.

## 5. Lazy executable quote i sticky proposal

Position-sized executable quote jest liczony wyłącznie po wyniku
`QuoteRequired`. `Hold` i przed-triggerowe `UnknownEvidence` nie uruchamiają
quote path.

Po pierwszym triggerze powstaje sticky proposal z:

- stabilnym `action_id`;
- `position_id` i epoką;
- oryginalnym reason;
- oczekiwaną remaining quantity;
- source snapshot ID;
- czasem triggera;
- bounded recovery deadline.

Nowy backward-compatible config:

```toml
[post_buy_guardian.exit_policy_v1]
quote_recovery_ms = 5000
```

W recovery window quote jest ponawiany najwyżej raz na tick i nie częściej niż
co wewnętrzny bounded retry interval. Proposal nie jest zastępowany nową
decyzją ekonomiczną. Quote jest obliczany poza lockiem; apply po quote ponownie
sprawdza revision, epoch, action ID i quantity.

`EffectiveExitPolicyV1Config` jest walidowany przed startem monitora, jawnie
serializowany przez `serde_json` i hashowany BLAKE3. Startup odrzuca:

- brak albo niefinitywny/ujemny take-profit;
- brak stop-loss albo wartość poza `0..=100%`;
- zerowy inactivity timeout;
- zerowy quote recovery;
- shadow bez pełnego albo włączonego Guardiana;
- AEM włączony w aktywnym profilu Position Manager Lite.

## 6. Shadow success i shadow Unknown

### 6.1 Resolved simulated close

Tylko poprawny full-position quote może doprowadzić do kolejności:

1. guarded state apply;
2. `ExitSubmitted`;
3. `ExitFilled { status: Filled }`;
4. `PositionClosed`;
5. canonical `ShadowTerminalTruthV2`;
6. `ShadowTerminalDisposition::SimulatedClosed`;
7. zwolnienie shadow slotu.

Shadow nie używa już on-chain statusu `Confirmed` w tej aktywnej ścieżce.

### 6.2 Brak, stale albo invalid truth po proposal

Quote failure:

- emituje lub deduplikuje `ExitBlocked`;
- nie zmniejsza quantity;
- nie emituje fill, close ani PnL;
- zachowuje sticky action;
- ponawia quote przez maksymalnie 5 s.

Po wyczerpaniu recovery pozycja kończy shadow symulację jako jedno z:

- `BlockedByData`;
- `NoFill`;
- `Failed`.

Powstają zgodne projekcje:

- `EventKind::ShadowPositionUnresolved`;
- lifecycle `position_unresolved`;
- Shadow V2 `TerminalBlocked`;
- `ShadowTerminalDisposition::SimulationBlocked`.

Unresolved nie posiada fill price ani PnL. Zachowuje remaining raw quantity,
action ID, policy identity/hash, source snapshot ID, recovery duration i
ostatnią truth provenance.

Brak/stale mark przed powstaniem price-triggered proposal nie może sam
wytworzyć TP/SL. Inactivity pozostaje warunkiem czasowym: może utworzyć
proposal, po czym nadal wymaga realizowalnego quote i podlega recovery.

### 6.3 Różna semantyka Unknown w shadow i live

Shadow `SimulationBlocked` zwalnia capacity, ponieważ symulacja nie oznacza
realnej ekspozycji on-chain. Utrata terminal channel ma osobny typed reason
`terminal_channel_dropped` i również zwalnia wyłącznie shadow reservation.

Dormant live `ExitConfirmationUnknown` działa odwrotnie:

- zachowuje submitted signature;
- zachowuje visible/remaining quantity;
- nie jest retryable;
- nie buduje kolejnego SELL;
- nie zamyka registry;
- nie zwalnia slotu;
- wymaga późniejszego reconciliation.

Live pozostaje nieaktywne. Progi dormant live `+58%/-46%` nie zostały
zmienione.

## 7. Authority Guardian, AEM i Revolvera

Aktywny launcher tworzy `SignalRouter::new_observation_only()` dla shadow i
probe.

LIGMA, WHF, TCF i PANIC nadal mogą liczyć i logować evidence, ale każdy sygnał
ma `consumed_by_policy=false`. Router nie wykonuje `TightenStop`, `PanicSell`
ani innej mutacji virtual magazine.

`ShadowPositionBook` jest mirror/diagnostic state, nie canonical position
authority. Jego brak jest naprawiany lub pomijany; nie może zamknąć
`MonitoredPosition`.

Aktywny TOML ustawia:

```toml
[post_buy_guardian.aem]
enabled = false
```

AEM code i paper/test compatibility pozostają w repo, lecz launcher nie
wstrzykuje AEM runtime do Position Manager Lite. Revolver worker oraz
pre-signed bullets nie są używane jako policy ani executor PR 1.

## 8. Config wiring, handoff i capacity

Launcher przekazuje cały `PostBuyGuardianConfig`. Runtime może nadpisać tylko
rzeczywisty `max_monitored_positions` oraz ustawić runtime-owned artifact paths
przez istniejące adaptery. Test porównuje pełną serializowaną strukturę po
overlayu, aby wykryć powrót do `default + kilka pól`.

Emitowany startup status zawiera:

- policy ID, version i config hash;
- lane;
- TP, SL i inactivity;
- quote recovery;
- Guardian authority `observation_only`;
- AEM i Revolver `disabled`;
- live `disabled`.

Direct handoff używa bounded `mpsc::channel` o pojemności:

```text
clamp(max_concurrent_positions * 4, 8, 256)
```

Producent używa `try_send`; nie czeka na hot path. `Full` i `Closed` mają
oddzielne metrics/reason. Nie ma fallbacku do ukrytej kolejki.

Po zaakceptowanym shadow handoff rejestracja zwraca prywatny terminal receiver
powiązany z dokładną pozycją i epoką. Launcher zwalnia slot z typed terminal
notification, nie z pollingu `get_position_health() == None`.

## 9. Event schema, replay i outcome contract

`EventKind::ShadowPositionUnresolved` jest addytywnym wariantem schema.
Historyczne eventy nadal się deserializują.

`EventValidator` wymaga dokładnie jednego terminala dla otwartej trajectory:

- `PositionClosed`; albo
- `ShadowPositionUnresolved`.

Unresolved:

- jest dozwolone tylko dla `Lane::Shadow`;
- wymaga wcześniejszego `PositionOpened`;
- wymaga position ID i epoch;
- nie może współistnieć z close;
- nie może twierdzić, że posiada authoritative net PnL.

Resolved lifecycle zapisuje addytywnie:

- policy ID/version/config hash;
- snapshot i action identity;
- entry/exit raw quantity;
- mark return i executable gross return;
- MFE/MAE mark;
- peak drawdown;
- absolute i inactivity age;
- quote source/slot/timestamp/age;
- reserve inputs i own impact;
- jawne `execution_cost_coverage=unmodeled`;
- terminal disposition/reason.

Ponieważ PR 1 nie modeluje jeszcze pełnych opłat/tipu, net PnL nie jest
oznaczany jako authoritative. Nie powstał Type-5 model ani expected-EV
predictor.

## 10. Celowo niezmieniony zakres

PR 1 nie zmienia:

- `MaterializedFeatureSet` ani prebuy materialization;
- Gatekeeper V2 policy, progów, reason codes lub BUY/REJECT/TIMEOUT;
- Gatekeeper V2.5, V3, Type-5, selectora ani IWIM;
- execution mode `shadow` ani entry mode `shadow_only`;
- dormant live sender activation;
- dormant live `+58%/-46%`;
- paper AEM compatibility;
- TimeStopV2/ShadowV2 diagnostic authority;
- partial quantity accounting;
- absolute max-hold i CrashGuard, które należą do osobnego PR 2.

## 11. Zmienione pliki

- `ghost-brain/src/guardian/post_buy/exit_policy_v1.rs` — pure policy,
  immutable snapshot, effective config/hash oraz guarded contracts;
- `ghost-brain/src/guardian/post_buy/engine.rs` — cutover aktywnego shadow,
  sticky proposal, lazy quote, resolved/unresolved terminalizacja, typed
  notification i outcome fields;
- `ghost-brain/src/guardian/post_buy/config.rs` — backward-compatible
  `ExitPolicyV1Config`;
- `ghost-brain/src/guardian/post_buy/integration.rs` — observation-only
  `SignalRouter`;
- `ghost-brain/src/guardian/post_buy/mod.rs` — wąski publiczny eksport statusu,
  configu i terminal receiver contract;
- `ghost-brain/src/events/schema.rs`, `mod.rs`, `validator.rs` — addytywny
  unresolved event i trajectory rules;
- `ghost-brain/ghost_brain_config.toml` — 5 s quote recovery i aktywne AEM
  disabled;
- `ghost-launcher/src/components/post_buy_runtime.rs` — full config wiring,
  bounded handoff, terminal watcher i live Unknown taxonomy;
- `ghost-launcher/src/main.rs` — pełny Guardian config i startup validation;
- `ghost-launcher/src/oracle_runtime.rs` — bounded `try_send` z typed queue
  reason;
- testy crate’ów — parity, lifecycle, schema, queue, wiring i dormant live
  contracts.

Plan `PLANS/DO_REALIZACJI/POSITION_MANAGER_LITE_V1.md` jest plikiem
dostarczonym przez właściciela i nie był modyfikowany w ramach implementacji.

## 12. Weryfikacja lokalna

Zaliczone po końcowym diffie:

- `cargo test -p ghost-brain guardian::post_buy --quiet -- --test-threads=1`
  — `171/171`;
- `cargo test -p ghost-brain guardian::post_buy::exit_policy_v1::tests --quiet`
  — `10/10`;
- `cargo test -p ghost-brain events::validator --quiet` — `8/8`;
- `cargo test -p ghost-launcher components::post_buy_runtime::tests --quiet --
  --skip shadow_v2_validation_smoke_marker_writes_required_artifacts_without_handoff`
  — `63/63`;
- `cargo test -p ghost-launcher --test post_buy_runtime_integration --quiet`
  — `4/4`;
- nowy test production TOML
  `post_buy_guardian_lifecycle_thresholds_load_from_production_toml` — `1/1`;
- `cargo test -p ghost-launcher --test gatekeeper_v25_regression --quiet`
  — `42/42`;
- `cargo test -p ghost-launcher --test gatekeeper_v3_tests --quiet` — `9/9`;
- `cargo check -p ghost-brain --quiet` — PASS;
- `cargo check -p ghost-launcher --quiet` — PASS;
- `cargo clippy -p ghost-brain -p ghost-launcher --lib --tests --quiet
  --message-format short -- -A clippy::never_loop
  -A clippy::absurd_extreme_comparisons` — PASS z istniejącymi ostrzeżeniami;
- `cargo fmt --all -- --check` — PASS;
- `git diff --check` — PASS.

Pełny Guardian suite wymagał uprzedniego usunięcia wyłącznie odtwarzalnego
`target/debug/incremental`, ponieważ filesystem miał `available=0`; po
odzyskaniu miejsca oba testy disk-headroom przeszły i wynik końcowy wyniósł
`171/171`.

### Rozpoznane baseline’y poza zakresem PR 1

Nie maskowano dwóch istniejących problemów:

1. pełny `ghost_brain_config_load_test` ma wynik `6/7`, ponieważ test na
   `origin/main` oczekuje `min_market_cap_sol = 5.0`, a aktywny TOML na tym
   samym base SHA zawiera `115.0`; nowy post-buy test przechodzi;
2. niezmieniony test
   `shadow_v2_validation_smoke_marker_writes_required_artifacts_without_handoff`
   nadal oczekuje density rows po samym `POSITION_CREATED`, mimo że aktualny
   compact-density contract flushuje je przy terminal truth. Funkcja testowa
   nie została zmieniona przez PR 1.

Pełne `clippy --all-targets` jest dodatkowo blokowane przez niezmienione pliki
baseline:

- `ghost-brain/src/pipeline/execution.rs` (`clippy::never_loop`);
- `ghost-brain/tests/mock_pump_amm.rs`
  (`clippy::absurd_extreme_comparisons`);
- `ghost-launcher/examples/oracle_pipeline_diagnostic.rs` (brak pola
  `creator_vault` w historycznym initializerze).

Wszystkie trzy pliki są identyczne z `origin/main`; dlatego uruchomiono i
zaliczono właściwy dla zmienianych crate’ów zakres `--lib --tests`.

## 13. Rollback i następny krok

Rollback PR 1 jest pełnym revertem całego PR. Nie należy przywracać starego i
nowego evaluatora równolegle ani zostawiać częściowego cutoveru terminalizacji.

Po review i CI następnym krokiem jest osobny PR 2 z zaakceptowanego planu:
absolute max-hold oraz minimalny CrashGuard V1. PR 2 może korzystać z
istniejącego snapshot/policy/apply contractu, ale nie powinien aktywować
partials, AEM, Guardian composite authority ani Revolver bullets.

Uwaga: wskazany przez globalną instrukcję szablon
`/Gho/docs/ADR/ADR_8D_SZABLON.md` nie występuje w tym checkoutcie. Zachowano
format stosowany przez lokalny korpus `docs/ADR/ADR_8D_*`.
