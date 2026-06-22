# ADR-8D: Post-Buy Lifecycle Thresholds Config

Status: IMPLEMENTED / TARGETED_TESTS_PASSED
Typ: ADR-8D / post-buy shadow lifecycle config, rollout safety, compatibility
Data: 2026-06-20
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: konfigurowalne progi `Target`, `StopLoss` i `TimeStop` dla shadow/probe post-buy lifecycle
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-brain/src/guardian/post_buy/config.rs`
- `ghost-brain/src/guardian/post_buy/engine.rs`
- `ghost-brain/src/guardian/post_buy/integration.rs`
- `ghost-brain/src/pipeline/builder.rs`
- `ghost-brain/tests/ghost_brain_config_load_test.rs`
- `ghost-launcher/src/components/post_buy_runtime.rs`
- `ghost-launcher/src/main.rs`
- `ghost-brain/ghost_brain_config.toml`
- `ghost-brain/ghost_brain_config.example.toml`
- `configs/rollout/ghost_brain_selector_dataset_sampler.toml`
- `configs/rollout/ghost_brain_selector_dataset_sampler_r41_score12_median_timeout_filters_maxwait31100_fsc_off.toml`
- `ghost-launcher/tests/post_buy_runtime_integration.rs`
- `docs/ADR/ADR_8D_POST_BUY_LIFECYCLE_THRESHOLDS_CONFIG_20260620.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ostatnich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Usunac koniecznosc przebudowy binarek po kazdej zmianie progow zamykania pozycji w shadow/probe lifecycle.

Wymagania:
- Dodac do `[post_buy_guardian]` pola konfiguracyjne:
  - `target_threshold` jako wartosc procentowa, np. `50.0` oznacza `+50%`.
  - `stoploss_threshold` jako wartosc procentowa, np. `50.0` oznacza `-50%`.
  - `wait_for_timestop` jako czas w milisekundach.
- `target_threshold` ma dopuszczac wartosci powyzej `100.0`.
- `wait_for_timestop` ma sterowac zamknieciem pozycji z reasonem `TimeStop`.
- Dotychczasowe configi bez nowych pol maja zachowac kompatybilnosc i legacy behavior.

## 2. Opis problemu - 3W2H

What:
Progi lifecycle byly rozproszone pomiedzy launcher configiem `live_exit_take_profit_pct` / `live_exit_stop_loss_pct` oraz stala czasu `SHADOW_POSITION_TIME_STOP_MS = 30_000`. W praktyce zmiana czasu `TimeStop` wymagalaa zmiany kodu i rebuild.

Where:
- shadow/probe runtime w `ghost-launcher/src/components/post_buy_runtime.rs`
- monitoring/shadow post-buy engine w `ghost-brain/src/guardian/post_buy/engine.rs`
- virtual magazine/shadow position book w `ghost-brain/src/guardian/post_buy/integration.rs`
- brain TOML configs z sekcja `[post_buy_guardian]`

Why:
Reruny selektora i probe wymagaja szybkiego strojenia progow `Target`, `StopLoss` i `TimeStop`. Te wartosci sa parametrami eksperymentu runtime, a nie logika wymagajaca rekompilacji.

How:
Dodano addytywne pola serde `Option` do `PostBuyGuardianConfig`, przepieto je przez launcher do `MonitoringEngine`, `ShadowPositionBook` i probe runtime oraz dodano testy konfiguracji i runtime conversion.

How many:
Zmiana dotyka tylko shadow/probe post-buy lifecycle. Nie zmienia Gatekeeper BUY policy, scoringu, DecisionLogger schema, TX buildera, sendera ani live inclusion path.

## 3. Przyczyna zrodlowa

Root cause:
Post-buy lifecycle threshold surface nie mial jednego jawnego miejsca w brain configu. Runtime uzywal czesciowo launcherowych `live_exit_*` fallbackow i czesciowo stalej kompilacyjnej dla czasu `TimeStop`.

Skutek:
- zmiana Target/StopLoss byla nieintuicyjna, bo pola mialy nazwy live-exit, mimo uzycia w shadow/probe;
- zmiana TimeStop wymagala rebuild;
- rollout profile nie dokumentowaly kompletu lifecycle parametrow obok `[post_buy_guardian]`.

## 4. Strategia naprawy

Przyjeta strategia:
- Dodac nowe pola do `PostBuyGuardianConfig` jako `Option`, aby stare TOML-e nadal sie ladowaly.
- W TOML-u wartosci trzymac w punktach procentowych, bo to jest naturalny interfejs operatorski:
  - `target_threshold = 50.0` -> Target przy `final_pnl_pct >= 50.0`.
  - `stoploss_threshold = 50.0` -> StopLoss przy `final_pnl_pct <= -50.0`.
  - `target_threshold = 150.0` -> Target przy `final_pnl_pct >= 150.0`.
- W runtime konwertowac percent points na fraction tylko tam, gdzie istniejacy engine oczekuje fraction.
- Dopuszczac target powyzej 100%.
- StopLoss sanitizowac do maksimum 100%, bo loss ponizej `-100%` nie ma sensu dla pozycji.
- `wait_for_timestop = 0` traktowac jak brak wartosci i wracac do legacy `30_000 ms`.
- Zachowac stary fallback `live_exit_take_profit_pct` / `live_exit_stop_loss_pct`, gdy nowe pola nie istnieja.

## 5. Przeprowadzone akcje naprawcze

Zmiana 1: config surface w `ghost-brain`
- Dodano do `PostBuyGuardianConfig`:
  - `target_threshold: Option<f64>`
  - `stoploss_threshold: Option<f64>`
  - `wait_for_timestop: Option<u64>`
- Dodano `DEFAULT_WAIT_FOR_TIMESTOP_MS = 30_000`.
- Dodano helper `wait_for_timestop_ms()`, ktory zachowuje legacy fallback.

Zmiana 2: shadow lifecycle runtime
- `MonitoringEngine` czyta timeout przez `config.wait_for_timestop_ms()`.
- `ShadowPositionBook` dostal `with_time_stop_ms()`.
- Virtual magazine config dostaje czas TimeStop wyliczony z ms do sekund z zaokragleniem w gore.
- Runtime snapshot retention uzywa tego samego konfigurowalnego timeoutu.

Zmiana 3: launcher bridge
- `PostBuyRuntimeConfig` dostal pola:
  - `shadow_target_threshold`
  - `shadow_stoploss_threshold`
  - `shadow_wait_for_timestop`
- `ghost-launcher/src/main.rs` przepina nowe pola z `ghost_brain_config.post_buy_guardian`.
- Shadow BUY runtime i probe runtime ustawiaja `ShadowSimpleExitThresholds` z nowych procentowych wartosci.
- Log startowy runtime pokazuje aktywne `target_threshold_pct`, `stoploss_threshold_pct` i `wait_for_timestop_ms`.

Zmiana 4: TOML profile
- Dodano jawne pola do:
  - `ghost-brain/ghost_brain_config.toml`
  - `ghost-brain/ghost_brain_config.example.toml`
  - `configs/rollout/ghost_brain_selector_dataset_sampler.toml`
  - `configs/rollout/ghost_brain_selector_dataset_sampler_r41_score12_median_timeout_filters_maxwait31100_fsc_off.toml`
- Wartosc domyslna w tych profilach pozostaje zgodna z dotychczasowym zachowaniem:
  - `target_threshold = 50.0`
  - `stoploss_threshold = 50.0`
  - `wait_for_timestop = 30000`

Zmiana 5: test coverage
- Dodano test deserializacji nowych pol z TOML.
- Dodano test kompatybilnosci legacy default `30_000 ms`.
- Dodano test, ze target moze byc powyzej `100%`.
- Dodano test, ze stoploss percent jest ograniczony do `100%`.
- Dodano test, ze `MonitoringEngine` uzywa skonfigurowanego TimeStop.
- Dodano testy ladowania produkcyjnego i R41 TOML.

## 6. Walidacja

| Walidacja | Wynik | Status |
|---|---|---|
| `cargo test -p ghost-brain post_buy_guardian_lifecycle_thresholds --test ghost_brain_config_load_test` | 2 passed | PASS |
| `cargo test -p ghost-brain --lib shadow_simple_exit_thresholds_allow_target_above_100_percent` | 1 passed | PASS |
| `cargo test -p ghost-brain --lib monitoring_engine_uses_configured_timestop_ms` | 1 passed | PASS |
| `cargo test -p ghost-launcher --lib shadow_exit_thresholds_use_post_buy_guardian_percent_fields` | 1 passed | PASS |
| `cargo test -p ghost-launcher --lib shadow_stoploss_percent_is_clamped_to_full_position_loss` | 1 passed | PASS |
| `git diff --check` | no whitespace errors | PASS |

Dodatkowa obserwacja:
Pelniejsze `cargo test -p ghost-launcher shadow_exit_thresholds_use_post_buy_guardian_percent_fields` probowalo zbudowac takze unrelated integration target `ghost-launcher/tests/gatekeeper_v25_regression.rs` i zatrzymalo sie na brakujacym polu `selector_soft_score` w lokalnych inicjalizatorach `GatekeeperDecision`. To nie pochodzi z tej zmiany lifecycle threshold.

## 7. Ryzyka i zabezpieczenia

Ryzyko 1: zmiana semantyki jednostek.
- Zabezpieczenie: TOML uzywa procentow, a nazwy komentarzy jasno mowia, ze `50.0` oznacza `50%`. Konwersja do fraction dzieje sie tylko wewnatrz launcher/runtime adaptera.

Ryzyko 2: target powyzej 100%.
- Decyzja: dozwolone zgodnie z wymaganiem. Sanitizer tylko odrzuca wartosci niefinite i wartosci ujemne.

Ryzyko 3: stoploss powyzej 100%.
- Decyzja: runtime ogranicza stoploss do `100%`, bo prog ponizej `-100%` nie reprezentuje sensownej straty pozycji.

Ryzyko 4: stare rollout configi bez nowych pol.
- Zabezpieczenie: pola sa `Option`, a runtime zachowuje stare fallbacki i domyslny `30_000 ms`.

Ryzyko 5: shadow/live boundary.
- Zabezpieczenie: zmiana jest w shadow/probe post-buy lifecycle. Nie wlacza live execution, nie zmienia sendera, TX buildera ani Gatekeeper BUY policy.

## 8. Decyzja

Post-buy lifecycle thresholds sa teraz konfigurowalne w `[post_buy_guardian]`:

```toml
target_threshold = 50.0
stoploss_threshold = 50.0
wait_for_timestop = 30000
```

Operator moze zmienic Target, StopLoss i TimeStop przez TOML bez rebuild. Stare configi pozostaja kompatybilne, a domyslne zachowanie pozostaje `+50%`, `-50%`, `30_000 ms`.
