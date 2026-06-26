# ADR-8D: R49 TimeStop V2 maxwait66000 rollout start

Status: RUNNING / TIMESTOP_V2_WINDOWS_CONFIRMED / PANIC_OBSERVED
Typ: ADR-8D / rollout config and shadow research run
Data: 2026-06-26
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `research/alpha-31100-validation-harness-v1`
Commit/PR: local runtime/config change, not committed at ADR creation time
Zakres: R48-derived shadow run z `max_wait_time_ms=66000`, `time_stop_v2_window` i `shadow_exit_replay_v1`
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `configs/rollout/ghost_brain_selector_dataset_sampler_r49_target60_stop60_exit_replay_timestop_v2_maxwait66000_fsc_off.toml`
- `configs/rollout/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1.toml`
- `docs/ADR/ADR_8D_R49_TIMESTOP_V2_MAXWAIT66000_RUN_START_20260626.md`

Powiazane runy/logi/raporty:
- Scope: `shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1`
- Expected lifecycle: `logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/shadow_lifecycle.jsonl`
- Expected probe lifecycle: `logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/probe_shadow_lifecycle.jsonl`
- Expected exit replay: `logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/shadow_exit_replay_v1.jsonl`
- Active tmux: `r49-timestop-v2-maxwait66000`
- Successful launcher report: `reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/run_lifecycle_guard_20260626T082624Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json`
- Runtime log: `reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/run_lifecycle_guard_20260626T082624Z/runtime.log`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ostatnich raportach.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Uruchomic nowy run oparty na R48 target60/stop60 exit replay, ale z dluzszym oknem obserwacji Gatekeepera `66000 ms` oraz dzialajacym `TimeStop V2` emitujacym `time_stop_v2_window`.

Rzeczywisty przebieg:
- Skopiowano R48/R2 wrapper i brain config do izolowanego R49 scope.
- Zmieniono namespace, sciezki artefaktow oraz porty metryk/gui.
- Zmieniono `max_wait_time_ms` z `31100` na `66000`.
- Dodano `[post_buy_guardian.time_stop_v2] enabled=true`.
- Zachowano `[post_buy_guardian.exit_replay_v1] enabled=true`.
- Pierwszy start guardem z `--event-canary-seconds 60` zostal zabity jako `FAIL_EVENT_CANARY`, bo przy oknie `66000 ms` nie zdazyl powstac candidate row (`Candidate_delta <= 0`), mimo aktywnych eventow.
- Drugi start guardem z `--event-canary-seconds 150` przeszedl `PASS` i zostawil runtime w tmux.
- Potwierdzono emisje `record_type=time_stop_v2_window` w `shadow_lifecycle.jsonl`.

Odchylenia od planu:
- R48/R2 nie mialo wlaczonego TimeStop V2, dlatego dla spelnienia celu "dzialajace `time_stop_v2_window`" konieczne bylo dodanie bloku TimeStop V2, poza sama zmiana okna obserwacji.
- Dla okna obserwacji `66000 ms` standardowy 60-sekundowy event canary jest za krotki; skuteczny start wymagal dluzszego canary.
- Runtime log zawiera pojedyncza panike `thread 'tokio-rt-worker' ... attempt to add with overflow`; proces pozostal zywy i dalej emitowal artefakty. Wymaga follow-up diagnostycznego.

## 2. Wykorzystane skills/sub-agenci

Nazwa: `ghost-execution`
Powod uzycia: rollout dotyka shadow runtime, Gatekeeper observation window i post-buy lifecycle evidence.
Zakres uzycia: ochrona SSOT, shadow/live boundary, no Gatekeeper policy rewrite.
Wynik: zmiana ograniczona do configow runu.
Ograniczenia: skill nie ocenia przyszlej wartosci sygnalu TimeStop V2.

Nazwa: `trading-systems`
Powod uzycia: run zbiera post-entry lifecycle/counterfactual evidence.
Zakres uzycia: potwierdzenie, ze to data-collection run, nie runtime promotion.
Wynik: no production promotion, no live execution.
Ograniczenia: brak oceny ekonomicznej bez zebranych artefaktow.

Nazwa: `rust-master`
Powod uzycia: dlugotrwaly async runtime w tmux.
Zakres uzycia: sprawdzenie procesu, portow, tmux i runtime health.
Wynik: start planowany przez lifecycle guard.
Ograniczenia: brak zmian kodu Rust.

## 3. Opis problemu - 3W2H

What:
Potrzebny jest nowy run, ktory zbiera jednoczesnie exit replay oraz TimeStop V2 windows. R48/R2 mial exit replay, ale zero `time_stop_v2_window`.

Where:
- rollout config
- brain config
- shadow/probe lifecycle logs

Why it matters:
`PR-TIMESTOP-V2-COUNTERFACTUAL-LAB-V1` wymaga scope, ktory ma jednoczesnie `shadow_exit_replay_v1.jsonl` i `time_stop_v2_window`, inaczej raport konczy sie `TIMESTOP_V2_NO_WINDOWS`.

How observed:
R48/R2 smoke counterfactual lab pokazal:
- `positions_with_exit_replay > 0`
- `positions_with_tsv2_windows = 0`
- `recommendation = TIMESTOP_V2_NO_WINDOWS`

How many / scale:
R49 ma zbierac szeroki threshold-probe dataset z `sample_modulus=1`, analogicznie do R48.

Evidence:
Nowy brain config ma `max_wait_time_ms=66000`, `[post_buy_guardian.time_stop_v2] enabled=true`, `[post_buy_guardian.exit_replay_v1] enabled=true`.

## 4. Przyczyna zrodlowa

Root cause:
R48/R2 byl exit-replay-only. Nie wlaczyl TimeStop V2 observe-only telemetry.

Mechanizm bledu:
Bez `time_stop_v2_window` nie da sie policzyc counterfactual exit przy candidate time.

Miejsce:
Config surface `[post_buy_guardian.time_stop_v2]`.

Skutek:
Offline lab nie moze ocenic TimeStop V2 na R48/R2.

Dowod:
R48/R2 lifecycle/probe lifecycle nie zawieral `record_type=time_stop_v2_window`.

Odrzucone hipotezy:
- Nie trzeba zmieniac TimeStop V2 engine.
- Nie trzeba zmieniac Gatekeeper policy.
- Nie trzeba wlaczac live execution.

## 5. Strategia naprawy

Przyjeta strategia:
Utworzyc izolowany R49 scope oparty na R48/R2, wlaczyc TimeStop V2 observe-only i wydluzyc Gatekeeper observation window do `66000 ms`.

Zakres ingerencji:
- Nowe configi rollout/brain.
- Nowy ADR.
- Runtime start przez `scripts/start_selector_lifecycle_run.py`.

Czego nie zmieniano:
- Rust runtime code.
- Gatekeeper thresholds poza dlugoscia okna `max_wait_time_ms`.
- BUY/REJECT semantics.
- execution/send path.
- FSC pozostaje disabled.

Ryzyka:
- Dlugie okno `66000 ms` zmniejszy cadence finalnych decyzji.
- TimeStop V2 windows pojawia sie dopiero po shadow/probe entry i post-buy monitoring.
- Brak BUY lifecycle moze wymusic zero-buy lifecycle allowance dla strict/data-collection run.

Odrzucone alternatywy:
- Mutowanie R48/R2 in place: odrzucone, bo mieszaloby artefakty.
- Wlaczanie live execution: odrzucone jako poza zakresem.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `configs/rollout/ghost_brain_selector_dataset_sampler_r49_target60_stop60_exit_replay_timestop_v2_maxwait66000_fsc_off.toml`
- Co zmieniono: utworzono R49 brain config z `max_wait_time_ms=66000`, TimeStop V2 observe-only i exit replay.
- Dlaczego: potrzebny scope z `time_stop_v2_window` i `shadow_exit_replay_v1`.
- Efekt: TOML parse PASS.

Zmiana 2:
- Plik/modul: `configs/rollout/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1.toml`
- Co zmieniono: utworzono izolowany wrapper config z nowym namespace, sciezkami artefaktow i portami `9136/8836`.
- Dlaczego: uniknac mieszania z R48/R2.
- Efekt: TOML parse PASS, porty wolne.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| TOML parse | `python3 - <<'PY' ... tomllib ... PY` | both configs parse | PASS | lokalny run |
| Config values | TOML inspection | `max_wait_time_ms=66000`, `time_stop_v2=true`, `exit_replay_v1=true` | PASS | lokalny run |
| Port check | `ss -ltnp | rg ':(9136|8836)'` | no listeners | PASS | lokalny run |
| Launcher static guard | `scripts/start_selector_lifecycle_run.py ... --event-canary-seconds 150 ...` | static guard + preflight PASS | PASS | `run_lifecycle_guard_20260626T082624Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json` |
| Event canary | same launcher run | Candidate/NewPool/PoolTransaction deltas present | PASS | `SELECTOR_EVENT_CANARY_PASS` |
| TimeStop V2 observe | `python3 scripts/check_time_stop_v2_observe.py ... --json` | `time_stop_v2_window_rows=105`, `candidate_positions=11` at check time | PASS | local check |
| Runtime continuity | `tmux ls`, `pgrep -af ghost-launcher` | tmux session and process alive | PASS_WITH_WARNING | runtime panic observed |

Wniosek walidacyjny:
R49 zostal uruchomiony i zostawiony w tmux. `time_stop_v2_window` dziala w aktywnych artefaktach. Status runtime nie jest czysto zdrowy, bo odnotowano pojedyncza panike worker thread; proces jednak nadal dziala i emituje dane.

Ograniczenia walidacji:
- Start nie rozstrzyga wartosci ekonomicznej TimeStop V2.
- Pojedyncza panika `attempt to add with overflow` wymaga osobnej diagnostyki, jezeli bedzie sie powtarzac albo wplynie na coverage.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: namespace isolation
- Co zabezpiecza: brak mieszania z R48/R2.
- Kiedy sie aktywuje: wszystkie output paths wskazuja R49 scope.
- Jak przetestowano: TOML/rg inspection.
- Co pozostaje poza zakresem: kasowanie starych artefaktow.

Guardrail 2:
- Typ: shadow/live boundary
- Co zabezpiecza: brak live execution.
- Kiedy sie aktywuje: wrapper utrzymuje `entry_mode="shadow_only"` i `execution_mode="shadow"`.
- Jak przetestowano: config inspection.
- Co pozostaje poza zakresem: runtime economic validation.

## Otwarte ryzyka / follow-up

- Zdiagnozowac panike `thread 'tokio-rt-worker' ... attempt to add with overflow`, jezeli pojawi sie ponownie lub coverage zacznie spadac.
- Po zebraniu danych uruchomic `scripts/time_stop_v2_counterfactual_lab.py` na R49.
