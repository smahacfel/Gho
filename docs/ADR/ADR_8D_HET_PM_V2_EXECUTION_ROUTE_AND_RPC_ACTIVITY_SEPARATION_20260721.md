# ADR-8D: HET-PM V2 — wykonywalna trasa MaxHold i RPC freshness

Data: 2026-07-21

Typ: ADR-8D / HET-PM V2 active shadow / route execution / AccountStateCore

Status: IMPLEMENTED RUNTIME CORRECTION; provenance lock deferred until final committed SHA

Plan: `PLANS/DO_REALIZACJI/POSITION_MANAGER_HET_V2.md`.

## D1. Problem

Review aktywnego shadow managera wskazał dwa błędy semantyczne. Po pierwsze,
`AbsoluteMaxHold` omijał `RouteUnsupported`; po migracji mógł więc zaakceptować
syntetyczny curve quote i zapisać `SimulatedFilled` na niewspieranej trasie.
Po drugie, read-only RPC refresh przechodził przez zwykły reducer account
write. Niezmienione bajty konta mogły zwiększać count aktywności, velocity i
heartbeat vitality, mimo że rynek nie wykonał żadnego nowego write.

Nie jest to live-fund risk, ponieważ `live_authority=false`. Są to jednak
fałszywe shadow exity i zafałszowane dane decyzji, więc wymagały runtime fixu.

## D2. Decyzja dla MaxHold

`RouteUnsupported` oraz `RouteUnknown` są nieomijalnymi blockerami
wykonawczymi. MaxHold może ominąć brak trajectory, vitality albo anchora, ale
nigdy brak wykonalnej trasy.

```text
MaxHold due + PumpCurveSupported + fresh matching quote
  -> QuoteRequired -> shared shadow executor -> simulated fill

MaxHold due + CurveCompletePumpSwapUnsupported / Unknown
  -> Blocked(RouteUnsupported / RouteUnknown) -> no proposal / no fill
```

Route jest sprawdzany w lattice, quote finalizerze i ostatnim V2-bound checku
shared executor-a. Zmiana route po materializacji nie może więc zmienić
prequote w fałszywy fill.

## D3. Decyzja dla RPC refresh

`CanonicalPoolState` rozdziela odtąd:

- observation freshness: `last_observed_*`, `observation_count`, source;
- rzeczywistą aktywność danych: `last_data_change_*`, `data_change_count`,
  canonical Geyser ordering i velocity.

`apply_rpc_refresh()` nie zmienia monotonic Geyser guard ani globalnego
watermarku. Dla identycznego `account_data_hash` aktualizuje wyłącznie
observation freshness. Dla zmienionych bajtów aktualizuje data-change state,
ale nadal nie fałszuje Geyser slot ani write-version.

Post-buy materializuje observation timestamp dla quote freshness i
data-change count dla timeline/vitality/activity. Identyczny odczyt RPC może
więc odświeżyć quote boundary, ale nie może stać się heartbeat rynku.

Ta sama separacja obowiązuje w `MonitoringEngine::remember_shadow_snapshot()`.
Jeżeli późniejszy snapshot ma identyczne dane ekonomiczne, ale nowszy
`timestamp_ms`, runtime aktualizuje wyłącznie `last_shadow_snapshot` używany
przez guarded quote resolution. Nie podbija `state_revision`, nie aktualizuje
peak i nie dokłada sample'u do trajectory. Dzięki temu MaxHold oraz już
rozpoczęta proposal V1 dostają świeży quote, bez udawania nowej aktywności
rynku dla trajectory lub vitality.

## D4. Regresje i CI

Dodano test end-to-end:

```text
authoritative_shadow_max_hold_on_unsupported_route_never_simulates_fill
```

Przechodzi on przez lattice, quote resolution, authority selection i shared
executor; wymaga braku proposal, terminal commit i `SimulatedFilled`, a
sidecar musi zapisać `Blocked(RouteUnsupported)`.

Dodano również test:

```text
unchanged_rpc_refresh_updates_quote_observation_without_vitality_activity
```

Łączy reducer, snapshot timeline, activity anchor i TimeStop. Potwierdza
nowy timestamp quote bez wzrostu `tx_count`, trajectory sample, activity
heartbeat ani vitality checkpoint.

Regresje końca ścieżki quote obejmują również:

```text
authoritative_het_v2_ignores_v1_take_profit_and_executes_v2_max_hold
authoritative_het_v2_finishes_a_preexisting_v1_proposal_without_replacing_it
```

Oba podają późniejszą obserwację z identycznymi danymi konta. Pierwszy wymaga
wykonania V2 `AbsoluteMaxHold`; drugi wymaga dokończenia istniejącej, sticky
proposal V1. Zabezpieczają więc odrębnie semantykę świeżego quote i zakaz
fabrykowania market activity.

Workflow uruchamia teraz Pythonowe testy analyzera/promotion toola, pełny
moduł `guardian::post_buy`, pełny moduł launchera `post_buy_runtime`, testy
line-scoped diff Clippy i dokładne regresje HET-PM. Diff Clippy porównuje
primary span z faktycznie zmienionymi liniami, nie z całym zmienionym plikiem.

Density smoke fixture został lokalnie uruchomiony na exact base
`aaec7382907efa4581f7a541dad2757dee978ce5` i na aktualnym HEAD; oba
wykonania przeszły. Nie dodano allowlisty ani wyłączenia testu.

## D5. Provenance lock

Poprzedni checked-in lock opisywał runtime observera i nie może zostać uznany
za lock active shadow managera. Został więc jawnie zdegradowany do
`calibration_pending`: nie daje promotion passu, ale zachowuje konfigurację
i progi jako szablon kolejnego locka. Nie wolno wpisywać ręcznie SHA ani hash
binarki z dirty worktree.

Po ustabilizowaniu kodu i utworzeniu finalnego commit SHA należy wykonać tylko
canonical `lock-criteria`: clean detached worktree, clean release build z
exact SHA, materializacja tool/config/binary hashy, zapis locked criteria i
dopiero potem dwa prospective runy.

Kolejny lock rozdziela dozwolone `v2_shadow_*` od zakazanych `v2_live_*` i
`live_authority_violation_count`. Aktywność shadow pozostaje widoczna, ale
zero-tolerance obowiązuje dla każdej live mutacji lub naruszenia live
authority. Przed tym krokiem nie wolno traktować obecnego szablonu jako
promotion evidence dla active shadow managera.

## D6. Inwarianty i rollback

- live authority pozostaje wyłączone;
- read-only RPC nie wysyła transakcji;
- Geyser ordering pozostaje canonical source dla account writes;
- MaxHold nie fabrykuje wykonania bez route;
- V2 używa wyłącznie shared guarded shadow executor-a.

Rollback nie wymaga migracji live state. Nie wolno przywrócić starego
zachowania przez uznawanie route unsupported za wykonywalny ani przez
traktowanie RPC context slot jako Geyser write.
