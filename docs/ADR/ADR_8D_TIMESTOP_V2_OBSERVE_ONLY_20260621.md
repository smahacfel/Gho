# ADR-8D: TimeStop V2 Observe-Only Vitality Telemetry

Status: IMPLEMENTED / TARGETED_TESTS_PASSED
Typ: ADR-8D / post-buy shadow lifecycle telemetry / TimeStop V2 experiment
Data: 2026-06-21
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: observe-only TimeStop V2 telemetry dla shadow/probe post-buy lifecycle
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-brain/src/guardian/post_buy/config.rs`
- `ghost-brain/src/guardian/post_buy/engine.rs`
- `ghost-brain/ghost_brain_config.toml`
- `ghost-brain/ghost_brain_config.example.toml`
- `configs/rollout/ghost_brain_selector_dataset_sampler_r44_timestop_v2_observe_maxwait31100_fsc_off.toml`
- `configs/rollout/shadow-burnin-v3-r44-timestop-v2-observe-target50-stop50-fsc-off-r1.toml`
- `scripts/check_time_stop_v2_observe.py`
- `docs/ADR/ADR_8D_TIMESTOP_V2_OBSERVE_ONLY_20260621.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ostatnich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Dodac TimeStop V2 jako mechanizm obserwacyjny, ktory mierzy zywnosc poola w kolejnych oknach po wejsciu, ale nie zmienia jeszcze realnych powodow zamkniecia pozycji.

Zatwierdzony tryb:
- `observe_only`
- `snapshot-only`
- pierwsze okno: `T0 + 3000 ms`
- kolejne okna: co `4000 ms`
- kandydat TimeStop V2 po `3` nieudanych oknach i wieku pozycji co najmniej `11000 ms`

Non-goals:
- brak zmiany `Target`, `StopLoss` i legacy `TimeStop`
- brak zmiany Gatekeeper policy
- brak zmiany `MaterializedFeatureSet`
- brak zmiany TX buildera, sendera, live execution albo shadow/live boundary
- brak aktywnego zamykania pozycji przez V2

## 2. Opis problemu - 3W2H

What:
Legacy `TimeStop` liczy cisze lub aktywnosc przez `shadow_market_activity`, a ta aktywnosc moze zostac odswiezona przez mikrotransakcje bez realnego postepu ceny, market capu, wolumenu albo bonding progress.

Where:
- `MonitoringEngine::tick()`
- per-position state w `MonitoredPosition`
- `shadow_lifecycle.jsonl` i `probe_shadow_lifecycle.jsonl`

Why:
Analizy R41/R42 pokazaly, ze czesc pozycji z `TimeStop` i `StopLoss` moze wynikac z pooli, ktore tylko "tykaja" mikrotransakcjami, a nie wykazuja zdrowego momentum. Potrzebny jest osobny, audytowalny sygnal vitality, zanim zaczniemy zmieniac semantyke zamykania.

How:
Dodano per-position `TimeStopV2State`, ktory porownuje snapshoty z wlasnym checkpointem V2 i klasyfikuje okna jako:
- `alive`
- `weak`
- `heartbeat`
- `stale_or_insufficient`

How many:
Zmiana dotyka tylko post-buy shadow lifecycle telemetry i config surface. Aktywne decyzje close reason pozostaja w dotychczasowym kodzie.

## 3. Przyczyna zrodlowa

Root cause:
Legacy `TimeStop` rozstrzyga na podstawie braku aktywnosci od ostatniego market activity anchor. Nie rozroznia aktywnosci sensownej od heartbeat/mikrotransakcji, ktore utrzymuja pool jako pozornie aktywny.

Skutek:
- `TimeStop` moze byc opozniany przez mikrotransakcje.
- `StopLoss` moze przejmowac czesc przypadkow, gdy pool przez chwile pozornie tyka, a potem nagle zrzuca cene.
- Analiza runtime nie miala jawnego counterfactual sygnalu, ktory powiedzialby: "V2 zamknalby te pozycje wczesniej".

## 4. Strategia naprawy

Przyjeta strategia:
- Dodac konfigurowalny blok `[post_buy_guardian.time_stop_v2]`, domyslnie disabled.
- W `observe_only` emitowac tylko addytywne rekordy `time_stop_v2_window`.
- Nie dotykac `shadow_market_activity` i legacy exit path.
- Nie ustawiac `last_force_exit_reason_code`, `CloseReason`, ani nie usuwac pozycji z runtime.
- Terminalne lifecycle rows moga dostac summary V2 tylko wtedy, gdy V2 faktycznie cos zaobserwowal.
- R44 jest osobnym profilem z V2 enabled, bazowo skopiowanym z R43/R41-like scope.

## 5. Przeprowadzone akcje naprawcze

Zmiana 1: config surface
- Dodano `TimeStopV2Mode`.
- Dodano `TimeStopV2Config`.
- Dodano `PostBuyGuardianConfig.time_stop_v2`.
- Default pozostaje `enabled = false`, `mode = "observe_only"`.
- Stare TOML-e zachowuja kompatybilnosc przez `#[serde(default)]`.

Zmiana 2: runtime telemetry
- Dodano `TimeStopV2State` per pozycja.
- Dodano checkpointy V2 oparte o `MarketSnapshot`.
- Dodano klasyfikacje okien:
  - `alive_meaningful_progress`
  - `low_vitality_no_meaningful_progress`
  - `micro_tx_heartbeat_no_price_progress`
  - `stale_or_missing_market_sample`
  - `mixed_failed_vitality_windows`
- Dodano `ShadowLifecycleRecordType::TimeStopV2Window`.
- Dodano addytywne pola `time_stop_v2_*` do lifecycle JSONL.

Zmiana 3: hook w `MonitoringEngine::tick()`
- V2 jest wywolywany po odswiezeniu snapshotu i przed legacy `run_shadow_runtime_tick`.
- V2 dziala tylko dla `Lane::Shadow`.
- V2 nie zamyka pozycji nawet po wykryciu kandydata.

Zmiana 4: rollout config
- Dodano disabled default block do:
  - `ghost-brain/ghost_brain_config.toml`
  - `ghost-brain/ghost_brain_config.example.toml`
- Dodano R44:
  - `configs/rollout/ghost_brain_selector_dataset_sampler_r44_timestop_v2_observe_maxwait31100_fsc_off.toml`
  - `configs/rollout/shadow-burnin-v3-r44-timestop-v2-observe-target50-stop50-fsc-off-r1.toml`
- R44 uzywa portow `9132` i `8832`.
- R44 ma `time_stop_v2.enabled = true`.

Zmiana 5: walidator artefaktow
- Dodano `scripts/check_time_stop_v2_observe.py`.
- Skrypt liczy:
  - liczbe `time_stop_v2_window`
  - liczbe kandydatow V2
  - rozklady statusow i subreasonow
  - close reason pozycji, ktore byly kandydatami V2

## 6. Walidacja

| Walidacja | Wynik | Status |
|---|---|---|
| `rustfmt --edition 2021 ghost-brain/src/guardian/post_buy/config.rs ghost-brain/src/guardian/post_buy/engine.rs` | wykonane | PASS |
| `cargo test -p ghost-brain --lib time_stop_v2` | 5 passed | PASS |
| `cargo test -p ghost-brain --lib shadow_runtime_time_stop` | 8 passed | PASS |
| `python3 -m py_compile scripts/check_time_stop_v2_observe.py` | no errors | PASS |
| TOML parse dla base/example/R44 wrapper/R44 brain | `toml_parse=ok` | PASS |
| R44 stale-name scan | brak `r43`, `R43`, `9131`, `8831` w nowych R44 plikach | PASS |
| `git diff --check` dla dotknietych plikow | no whitespace errors | PASS |

## 7. Ryzyka i zabezpieczenia

Ryzyko 1: przypadkowa zmiana aktywnego close behavior.
- Zabezpieczenie: V2 nie wywoluje exit runtime, nie ustawia close reason, nie usuwa pozycji i nie zmienia legacy trigger priority.

Ryzyko 2: shadow/live boundary.
- Zabezpieczenie: V2 jest telemetry-only i dziala w shadow lane. Nie wlacza live execution.

Ryzyko 3: schema compatibility.
- Zabezpieczenie: pola JSONL sa addytywne i opcjonalne. `time_stop_v2_window` jest nowym record type, a stare terminalne recordy pozostaja obecne.

Ryzyko 4: snapshot-only ograniczenia.
- Zabezpieczenie: to celowo pierwszy etap. Brak buyer/signer flow w tej iteracji jest jawny i nie jest traktowany jako pelna analiza flow.

Ryzyko 5: staged/dirty worktree.
- Obserwacja: repo bylo juz brudne, a `ghost-brain/ghost_brain_config.toml` mial status `MM`. Nie wykonywano stagingu, commita ani resetu.

## 8. Decyzja

TimeStop V2 zostaje zaimplementowany jako observe-only vitality telemetry. Najblizszy runtime powinien uzyc R44 scope, a po zebraniu danych nalezy uruchomic:

```bash
scripts/check_time_stop_v2_observe.py \
  --shadow-lifecycle logs/shadow_run/shadow-burnin-v3-r44-timestop-v2-observe-target50-stop50-fsc-off-r1/shadow_lifecycle.jsonl \
  --probe-lifecycle logs/shadow_run/shadow-burnin-v3-r44-timestop-v2-observe-target50-stop50-fsc-off-r1/probe_shadow_lifecycle.jsonl \
  --json
```

Decyzja o przejsciu z `observe_only` do aktywnego shadow-close wymaga osobnego planu i porownania kandydatow V2 z realnymi `Target`, `StopLoss`, `TimeStop`.
