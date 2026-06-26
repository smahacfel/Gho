# ADR-8D: Shadow exit replay v1 research sidecar

Status: IMPLEMENTED / TARGETED_VALIDATION_COMPLETED
Typ: ADR-8D / post-buy shadow replay evidence
Data: 2026-06-25
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `research/alpha-31100-validation-harness-v1`
HEAD podczas pracy: `0cb800a`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: kompaktowy, pasywny log `shadow_exit_replay_v1.jsonl` oraz offline evaluator dla wielu target/stop bez osobnych rolloutow
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-brain/src/guardian/post_buy/config.rs`
- `ghost-brain/src/guardian/post_buy/engine.rs`
- `ghost-brain/src/guardian/post_buy/exit_replay.rs`
- `ghost-brain/src/guardian/post_buy/mod.rs`
- `ghost-launcher/src/components/post_buy_runtime.rs`
- `ghost-launcher/src/main.rs`
- `ghost-launcher/tests/post_buy_runtime_integration.rs`
- `scripts/shadow_exit_replay_eval.py`
- `scripts/test_shadow_exit_replay_eval.py`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje istniejacy lokalny format ADR-8D uzyty juz w repo.

## 1. Przygotowanie i dzialania wstepne

Cel:
Umozliwic offline przeliczanie wielu wariantow target/stop dla tej samej shadow pozycji bez uruchamiania oddzielnego rollout runu dla kazdej pary progow i bez logowania pelnego raw tick streamu.

Warunki brzegowe:
- zero zmian aktywnego BUY/REJECT,
- zero zmian Gatekeeper thresholds,
- zero zmian `v25_confidence`,
- zero wpiecia do selectora, alpha ani ML,
- zero zmian active shadow close semantics i live sell logic,
- jeden kompaktowy rekord JSONL per shadow entry,
- default `enabled = false`.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: ochrona shadow/live boundary, DecisionLogger/replay semantics i aktywna sciezka runtime.
- `rust-master`: implementacja bounded trackera, async shutdown behavior i testy Rust.
- `trading-systems`: rozdzielenie research evidence od decyzji/exit policy.
- `statistical-research-engine`: offline evaluator i target/stop replay semantics.

Zaladowane dokumenty specjalistyczne:
- `docs/agents/decision-logging-replay-analyst.md`
- `docs/agents/config-rollout-safety-reviewer.md`

Powod:
Zmiana dotyka JSONL/replay evidence, config defaults i shadow runtime. Nie dotyka Gatekeeper policy, Solana live execution ani selector/alpha scoring.

## 3. Opis problemu - 3W2H

What:
Dotychczas pojedynczy shadow run materializowal finalny wynik pozycji dla jednej konfiguracji target/stop. To nie wystarcza do offline analizy wariantow takich jak `+4/-3`, `+10/-1`, `+20/-5`, `+30/-10`, `+75/-7` bez ponownego uruchamiania bota.

Where:
Post-buy Guardian shadow monitoring path.

Why it matters:
Bez kompaktowej sciezki exit replay trzeba albo:
- odpalac osobny run per target/stop,
- albo logowac pelny raw stream,
- albo zgadywac z pojedynczego `final_pnl_pct`.

Kazda z tych opcji jest slaba: pierwsza jest kosztowna, druga grozi ogromnymi logami, trzecia nie wystarcza do wiarygodnego target/stop replay.

How observed:
W analizie R47/R48 finalne `close_reason` i `final_pnl_pct` byly wystarczajace do oceny jednego profilu, ale niewystarczajace do symulacji innych exitow.

How many / scale:
Jeden rekord per shadow entry, bounded `path_bps` do `max_path_points`, exact `first_hit_ms` dla stalej siatki poziomow.

## 4. Przyczyna zrodlowa

Root cause:
Istniejacy shadow lifecycle log jest kontraktem wyniku pozycji dla aktywnego profilu, nie kontraktem kompaktowego replay path. Brakowalo pasywnego artefaktu pomiedzy pelnym streamem tick-by-tick a pojedynczym finalnym PnL.

## 5. Strategia naprawy

Przyjeta strategia:
- Dodac `ShadowExitReplayTracker` jako niezalezny research sidecar w PostBuy Guardian.
- Tracker ma wlasna mape aktywnych trackerow i nie jest czescia `ShadowPositionBook`.
- Tracker obserwuje snapshoty pasywnie i zapisuje tylko bounded evidence.
- `horizon_ms` liczony jest od `entry_ts_ms`, nie od startu trackera.
- `first_hit_ms`, `mfe_bps` i `mae_bps` sa liczone ze wszystkich obserwowanych probek, niezaleznie od truncation `path_bps`.
- `path_bps` jest kompresowane przez `pnl_step_bps`, `heartbeat_ms`, hit nowego poziomu lub final sample.
- Shutdown nie blokuje procesu domyslnie: `flush_on_shutdown = false`.
- Przy shutdown przed horizon emitowany jest rekord `quality = "degraded"`, `reason = "shutdown_before_horizon"` z dostepnym stanem.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: config additive
- Dodano `ShadowExitReplayConfig` pod `PostBuyGuardianConfig`.
- Domyslnie `enabled = false`.
- Domyslne parametry:
  - `horizon_ms = 120000`,
  - `pnl_step_bps = 25`,
  - `heartbeat_ms = 1000`,
  - `max_path_points = 512`,
  - `flush_on_shutdown = false`,
  - `shutdown_flush_budget_ms = 3000`.
- Config ma `#[serde(default)]`, wiec stare konfiguracje pozostaja ladowalne.

Zmiana 2: tracker
- Dodano `ghost-brain/src/guardian/post_buy/exit_replay.rs`.
- Rekord schemy: `shadow_exit_replay_v1`.
- Dla valid entry zapisuje:
  - identity fields,
  - entry metadata,
  - `levels_bps`,
  - `first_hit_ms`,
  - `mfe_bps` / `mae_bps`,
  - `time_to_mfe_ms` / `time_to_mae_ms`,
  - `last_pnl_bps`,
  - bounded `path_bps`,
  - sample/path counters,
  - `quality` i opcjonalny `reason`.
- Invalid entry price daje `quality = "unavailable"`, `reason = "invalid_entry_price"`.
- Brak price path daje `quality = "unavailable"`, `reason = "no_price_path"`.

Zmiana 3: pasywne wpiecie w MonitoringEngine
- Tracker rejestrowany jest tylko dla `Lane::Shadow`.
- Sidecar ma osobne trackery i osobny log path.
- Active position lifecycle, slot release, close reason, `ShadowPositionBook` i runtime exits nie sa zmieniane.
- Gdy pozycja znika z active position book przed horizon, tracker moze dalej probowac obserwowac aktualny shadow snapshot az do horizon albo shutdown.
- Wpis JSONL trafia do `shadow_exit_replay_v1.jsonl` obok `shadow_lifecycle.jsonl`.

Zmiana 4: launcher wiring
- `PostBuyRuntimeConfig` dostal opcjonalne `shadow_exit_replay_v1`.
- Main przenosi config z `[post_buy_guardian.exit_replay_v1]`.
- Active shadow monitor ustawia log path tylko gdy replay jest enabled.
- Probe path nie zostal rozszerzony w tym PR, zeby ograniczyc blast radius.

Zmiana 5: offline evaluator
- Dodano `scripts/shadow_exit_replay_eval.py`.
- Dla target/stop bedacych w `levels_bps` evaluator uzywa exact `first_hit_ms`.
- Dla progow spoza siatki uzywa `path_bps` i oznacza wynik `result_quality = "path_approx"`.
- Ties sa konserwatywnie klasyfikowane jako `StopLoss`.
- TimeStop uzywa `last_pnl_bps`.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda | Wynik | Status |
|---|---|---|---|
| Rustfmt touched Rust files | `rustfmt --edition 2021 ghost-brain/src/guardian/post_buy/config.rs ghost-brain/src/guardian/post_buy/engine.rs ghost-brain/src/guardian/post_buy/exit_replay.rs ghost-brain/src/guardian/post_buy/mod.rs ghost-launcher/src/components/post_buy_runtime.rs ghost-launcher/src/main.rs ghost-launcher/tests/post_buy_runtime_integration.rs` | passed | PASS |
| Tracker unit tests | `CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-brain exit_replay::tests -- --nocapture` | 4 passed | PASS |
| Config serde defaults/overrides | `CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-brain deserialize_exit_replay_v1_defaults_and_overrides -- --nocapture` | 1 passed | PASS |
| Launcher post-buy wiring | `CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher post_buy_runtime -- --nocapture` | 40 lib tests, 2 integration tests, 3 invariant tests passed in filtered run | PASS |
| Evaluator tests | `python3 scripts/test_shadow_exit_replay_eval.py` | 3 passed | PASS |

Acceptance cases covered:
- `0 -> +500 -> -300`: positive level first hits before negative hits.
- `0 -> -300 -> +500`: evaluator classifies `+400/-300` as `StopLoss`.
- no target/stop hit: evaluator returns `TimeStop` with `last_pnl_bps`.
- `path_bps` does not exceed `max_path_points`.
- final sample is preserved under truncation.
- invalid entry price returns `quality = "unavailable"`.
- exact grid thresholds use `first_hit_ms`.
- non-grid thresholds use `path_bps` and `result_quality = "path_approx"`.

## 8. Krotki raport operacyjny

Gdzie zapisuje sie JSONL:
- active shadow: obok `shadow_lifecycle.jsonl`, jako `shadow_exit_replay_v1.jsonl`;
- launcher wyprowadza sciezke przez `derive_shadow_exit_replay_log_path()`;
- zapis powstaje tylko gdy `post_buy_guardian.exit_replay_v1.enabled = true`;
- default pozostaje disabled, wiec ten PR nie wlacza nowego artefaktu w istniejacych profilach.

Path size:
- limit runtime: `max_path_points = 512`;
- testowy fixture `/tmp/shadow_exit_replay_fixture.jsonl`: 3 rekordy, srednio 4.33 punktu, max 5 punktow;
- nie uruchamiano nowego rollout runu tylko po to, aby wygenerowac produkcyjny rozklad rozmiaru JSONL.

Przykladowy rekord, skrocony:

```json
{
  "schema": "shadow_exit_replay_v1",
  "run_id": "fixture",
  "session_id": "session-a",
  "pool_id": "pool-a",
  "base_mint": "mint-a",
  "entry_ts_ms": 1000,
  "entry_price": 1.0,
  "entry_source": "shadow_simulated",
  "horizon_ms": 120000,
  "close_age_ms": 120000,
  "levels_bps": [-5000, -3000, -2000, -1500, -1000, -700, -500, -300, -200, -100, 100, 200, 300, 400, 500, 700, 1000, 1500, 2000, 3000, 5000, 7500, 10000],
  "first_hit_ms": {"1000": 500, "3000": 1200, "-100": 9000, "-700": 10000},
  "mfe_bps": 3500,
  "mae_bps": -1200,
  "time_to_mfe_ms": 1200,
  "time_to_mae_ms": 10000,
  "last_pnl_bps": -500,
  "path_points_written": 5,
  "truncated": false,
  "quality": "clean"
}
```

Testowy eval na fixture:

| Variant | result_quality | total | target | stop | timestop | avg_pnl_bps | median_pnl_bps | sum_pnl_bps |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `+10/-1` (`1000/-100`) | exact_levels | 3 | 1 | 1 | 1 | 350.0 | 150.0 | 1050 |
| `+30/-10` (`3000/-1000`) | exact_levels | 3 | 1 | 0 | 2 | 1116.67 | 200.0 | 3350 |
| `+75/-7` (`7500/-700`) | exact_levels | 3 | 0 | 1 | 2 | -116.67 | 150.0 | -350 |

## 9. Ryzyka resztkowe

- Ten PR nie uruchamia `exit_replay_v1` w zadnym rollout profilu; default pozostaje disabled.
- Probe replay zostal swiadomie pominiety w pierwszym PR.
- `path_bps` jest approximation surface dla progow spoza `levels_bps`; dokladnosc gwarantowana jest tylko dla poziomow obecnych w `first_hit_ms`.
- Jakosc `clean` zalezy od obecnosci post-entry price samples w shadow source; brak sciezki jest jawnie `unavailable`.
- Istniejacy workspace zawiera unrelated dirty files/log deletions poza zakresem tej zmiany.

## 10. Scope out

Poza zakresem:
- Gatekeeper, `v25_confidence`, selector, alpha,
- BUY/REJECT semantics,
- active shadow close semantics,
- live sell logic,
- ML/XGBoost/research harness,
- runtime target/stop tuning,
- full raw price stream logging.

## 11. Decyzja

Przyjeto pasywny, default-disabled `shadow_exit_replay_v1` sidecar jako najnizszego ryzyka sposob na offline target/stop replay. Artefakt jest research-only i nie jest konsumowany przez aktywna decyzje runtime.
