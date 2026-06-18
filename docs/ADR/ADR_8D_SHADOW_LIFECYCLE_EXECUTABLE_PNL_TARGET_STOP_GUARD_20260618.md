# ADR-8D: Shadow lifecycle TARGET/STOP wymagaja executable PnL

Status: WIP / implemented locally, not committed
Typ: Correctness fix / shadow lifecycle anti-regression guard
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: /root/Gho / codex/gatekeeper-edge-policy-redesign-r1
Commit/PR: local changes on top of 4a4e6e4; no commit / no PR
Zakres: Shadow post-buy lifecycle closure semantics for TARGET/STOP business thresholds
Dotkniete moduly/pliki:
- ghost-brain/src/guardian/post_buy/engine.rs
- docs/ADR/ADR_8D_SHADOW_LIFECYCLE_EXECUTABLE_PNL_TARGET_STOP_GUARD_20260618.md
Powiazane runy/logi/raporty:
- configs/rollout/ghost_brain_selector_dataset_sampler_r34_maxwait2999_fsc_off.toml
- configs/rollout/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1.toml
- logs/shadow_run/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1/shadow_lifecycle.jsonl
- logs/shadow_run/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1/probe_shadow_lifecycle.jsonl
- logs/shadow_run/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1-buys.jsonl
- logs/shadow_run/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1/probe_transport.jsonl
Poziom ryzyka: Medium

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zweryfikowac, czy R34 zostal skonfigurowany dla okna obserwacji 2999 ms oraz business thresholds TARGET +50% / STOP -50%, zostawic R34 w tmux, a nastepnie sprawdzic, czy aktualne shadow lifecycle faktycznie respektuje te progi.

Rzeczywisty przebieg:
R34 zostal uruchomiony w tmux na profilu `shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1`. Config potwierdzil `max_wait_time_ms = 2999`, `live_exit_take_profit_pct = 0.50` i `live_exit_stop_loss_pct = 0.50`. W trakcie kontroli artefaktow wykryto, ze dzialajaca binarka potrafila zapisac `position_closed` z `close_reason=Target` przy `final_pnl_pct` ponizej 50%. To oznaczalo, ze wartosci configu byly ustawione poprawnie, ale runtime zamykal pozycje po przekroczeniu progu spot/price-ratio zanim executable sell truth potwierdzil business PnL.

Odchylenia od planu:
Zamiast tylko potwierdzic konfiguracje, trzeba bylo wprowadzic lokalny guard w shadow post-buy lifecycle. R34 nie zostal restartowany po tej poprawce, wiec aktualnie dzialajacy proces nie jest dowodem runtime dla nowego kodu.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia: Zmiana dotyka shadow-only lifecycle, audytowalnosci decyzji po BUY oraz granicy shadow/live.
Zakres uzycia: Potwierdzenie, ze zmiana nie modyfikuje Gatekeeper policy, SSOT `MaterializedFeatureSet`, execution/send path ani live behavior.
Wynik: Zmiana zostala ograniczona do `PostBuyGuardian` shadow lifecycle closure guard.
Ograniczenia: Skill nie zastepuje runtime walidacji na nowej binarce.

Nazwa: rust-master
Powod uzycia: Zmiana jest w asynchronicznym runtime Rust i musi nie wprowadzac ukrytej mutacji ani niestabilnych sciezek.
Zakres uzycia: Waski helper + unit test zamiast szerokiego refaktoru.
Wynik: Testy jednostkowe przechodza po `rustfmt`.
Ograniczenia: Nie uruchamiano pelnego workspace test suite.

Nazwa: solana-pumpfun-architect
Powod uzycia: Problem dotyczy roznicy miedzy spot trigger a ekonomicznie wykonywalnym sell PnL na pump.fun curve.
Zakres uzycia: Wymuszenie potwierdzenia TARGET/STOP na `PriceTruthResolver::resolve_shadow_exit`, a nie tylko na progu spot.
Wynik: TARGET/STOP w shadow lifecycle wymagaja executable PnL po symulowanym wyjsciu.
Ograniczenia: Nie usuwa residual RPC/route simulation failures typu `Custom(6002)` albo `Custom(2006)`.

## 3. Opis problemu -- 3W2H

What:
R34 mial business thresholds TARGET +50% i STOP -50%, ale shadow lifecycle mogl zapisac `Target` przy `final_pnl_pct` ponizej +50%.

Where:
`ghost-brain/src/guardian/post_buy/engine.rs`, sciezka `MonitoringEngine::run_shadow_simple_threshold_tick`.

Why it matters:
Pliki `shadow_lifecycle.jsonl` i `probe_shadow_lifecycle.jsonl` sa pozniej traktowane jako ekonomiczna prawda dla labeli TARGET/STOP/TIMEOUT. Jezeli `Target` moze zostac zapisany ponizej progu biznesowego, to dataset, segment lab i downstream validation dostaja falszywie pozytywne etykiety.

How observed:
Kontrola R34 wykazala config 50/50, ale w `shadow_lifecycle.jsonl` byly rekordy `position_closed` z `close_reason=Target` oraz `final_pnl_pct` ok. 49.5-49.8%.

How many / scale:
W probce R34 przed poprawka wykryto 4 rekordy `Target` ponizej +50% w BUY lifecycle. Probe lifecycle w tej samej probce mial wtedy tylko `TimeStop`, bez falszywego TARGET.

Evidence:
- `configs/rollout/ghost_brain_selector_dataset_sampler_r34_maxwait2999_fsc_off.toml`: `max_wait_time_ms = 2999`
- `configs/rollout/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1.toml`: `live_exit_take_profit_pct = 0.50`, `live_exit_stop_loss_pct = 0.50`
- `logs/shadow_run/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1/shadow_lifecycle.jsonl`: `Target` rows ponizej `final_pnl_pct = 50`

## 4. Przyczyna zrodlowa

Root cause:
Shadow simple exit trigger byl oparty na progu ceny/spot ratio, a dopiero pozniej liczyl executable exit truth. Brakowalo guardu, ktory odrzuca `TakeProfit` albo `StopLoss`, jezeli executable PnL po symulowanym sell nie spelnia business threshold.

Mechanizm bledu:
Rynek mogl przekroczyc spot threshold, ale executable sell po fee/slippage/curve impact dawalo PnL ponizej wymaganego +50%. Runtime mimo to zamykal pozycje jako `Target`, poniewaz trigger zostal juz zaakceptowany przed walidacja executable PnL.

Miejsce:
`MonitoringEngine::run_shadow_simple_threshold_tick` po `resolve_shadow_exit`, przed `set_shadow_exit_reason_code` i `execute_shadow_exit`.

Skutek:
Mozliwe falszywe `Target`/`StopLoss` w shadow lifecycle wzgledem business threshold.

Dowod:
R34 mial skonfigurowane +50/-50, a artefakt lifecycle zawieral `Target` z `final_pnl_pct` ponizej 50%.

Odrzucone hipotezy:
- Bledny config progow: odrzucone, bo config zawiera `0.50/0.50`.
- Problem tylko w parserze raportu: odrzucone, bo wartosci `final_pnl_pct` sa zapisane w samym lifecycle.
- Problem Gatekeeper policy: odrzucone, bo zjawisko wystepuje po BUY/probe w post-buy lifecycle, nie przy decyzji Gatekeepera.

## 5. Strategia naprawy

Przyjeta strategia:
Po `resolve_shadow_exit` sprawdzac executable `truth.pnl_pct` przeciw progom biznesowym. `TakeProfit` wymaga `truth.pnl_pct >= take_profit_pct * 100`, `StopLoss` wymaga `truth.pnl_pct <= -stop_loss_pct * 100`. Jezeli spot trigger zaszedl, ale executable truth nie potwierdza progu, zapisac `exit_blocked` i zostawic pozycje otwarta.

Zakres ingerencji:
Waska zmiana w shadow post-buy lifecycle i unit tests w tym samym module.

Czego nie zmieniano:
- Gatekeeper policy
- decision thresholds poza istniejacym configiem R34
- MaterializedFeatureSet / SSOT
- DecisionLogger schema
- execution/send path
- live execution
- route materialization
- shadow dispatch/probe eligibility

Ryzyka:
Pozycje moga dluzej pozostawac otwarte po spot-only triggerze i dojsc do pozniejszego `TimeStop`, jezeli executable PnL nigdy nie spelnia progu. To jest zamierzony efekt dla poprawnej etykiety biznesowej, ale wymaga runtime obserwacji na nowej binarce.

Odrzucone alternatywy:
- Obnizenie progu albo tolerancja raportowa: odrzucone, bo ukryloby blad etykiet.
- Liczenie TARGET tylko offline po lifecycle: odrzucone, bo zostawia runtime lifecycle z falszywym `close_reason`.
- Zmiana Gatekeepera: odrzucone, bo problem jest w post-buy shadow closure, nie w decyzji BUY/REJECT.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `ghost-brain/src/guardian/post_buy/engine.rs`
- Co zmieniono: Dodano helpery `take_profit_pnl_pct`, `stop_loss_pnl_pct`, `shadow_simple_exit_truth_satisfies_trigger` i `shadow_simple_exit_threshold_miss_evidence`.
- Dlaczego: Potrzebny byl jawny business-threshold check na executable PnL.
- Efekt: `TakeProfit` i `StopLoss` nie moga zamknac shadow position, jezeli `PriceTruthResolver` nie potwierdzi progu PnL.

Zmiana 2:
- Plik/modul: `ghost-brain/src/guardian/post_buy/engine.rs`
- Co zmieniono: W `run_shadow_simple_threshold_tick` dodano guard po `resolve_shadow_exit`; przy niespelnieniu progu zapisuje `exit_blocked`, loguje warning i wraca bez `position_closed`.
- Dlaczego: `exit_blocked` zachowuje audytowalnosc i nie falszuje `Target`/`StopLoss`.
- Efekt: Spot-only crossing nie tworzy falszywego lifecycle close reason.

Zmiana 3:
- Plik/modul: `ghost-brain/src/guardian/post_buy/engine.rs`
- Co zmieniono: Dodano test `shadow_runtime_take_profit_requires_executable_pnl_threshold` i skorygowano fixture happy-path take-profit tak, aby executable PnL faktycznie spelnial prog po fee/curve impact.
- Dlaczego: Test ma chronic przed regresja, w ktorej spot threshold udaje business TARGET.
- Efekt: Unit tests potwierdzaja happy path i negative guard.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Config R34 | `grep -n "live_exit_take_profit_pct\\|live_exit_stop_loss_pct\\|max_wait_time_ms\\|funding_lane_mode" configs/rollout/ghost_brain_selector_dataset_sampler_r34_maxwait2999_fsc_off.toml configs/rollout/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1.toml` | `max_wait_time_ms = 2999`, `live_exit_take_profit_pct = 0.50`, `live_exit_stop_loss_pct = 0.50`, `funding_lane_mode = "disabled"` | PASS | Config paths listed above |
| Runtime process | `pgrep -af "ghost-launcher|shadow-burnin-v3-r34|maxwait2999|r33"` | R34 running in tmux; no active R33 launcher in matched output | PASS | PID 2876428 `target/release/ghost-launcher --config ...r34...` |
| Unit negative guard | `cargo test -p ghost-brain shadow_runtime_take_profit_requires_executable_pnl_threshold -- --nocapture` | `1 passed; 0 failed` | PASS | Test asserts `exit_blocked` and no `position_closed Target` |
| Unit shadow simple threshold suite | `cargo test -p ghost-brain shadow_runtime_simple_threshold -- --nocapture` | `2 passed; 0 failed` | PASS | TakeProfit and StopLoss simple threshold tests pass |
| Formatting | `rustfmt --edition 2021 ghost-brain/src/guardian/post_buy/engine.rs` | no formatting error | PASS | Command completed successfully |
| Replay/simulation | Current R34 runtime | Current R34 was started before this code change and was not restarted | N/A | Runtime proof requires rebuilt binary and fresh/restarted run |

Wniosek walidacyjny:
Business thresholds sa poprawnie ustawione w R34 configu. Kodowo dodano i przetestowano guard, ktory wymaga executable PnL dla `Target`/`StopLoss`. Obecnie dzialajacy R34 nie jest dowodem tej poprawki, bo zostal uruchomiony przed przebudowa i restartem binarki.

Ograniczenia walidacji:
Nie wykonano pelnego `cargo test`. Nie zatrzymano ani nie restartowano R34 po poprawce. Nie usunieto residual shadow simulation failures typu `Custom(6002)`, `Custom(2006)` lub route materialization errors.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: Runtime guard
- Co zabezpiecza: Falszywe `Target`/`StopLoss` po spot-only crossing bez executable PnL.
- Kiedy sie aktywuje: Po `resolve_shadow_exit`, przed wykonaniem shadow exit, dla `TakeProfit` i `StopLoss`.
- Jak przetestowano: `shadow_runtime_take_profit_requires_executable_pnl_threshold`.
- Co pozostaje poza zakresem: Route materialization failures i RPC simulation failures przed lifecycle.

Guardrail 2:
- Typ: Audit evidence
- Co zabezpiecza: Mozliwosc rekonstrukcji, dlaczego spot trigger nie zamknal pozycji.
- Kiedy sie aktywuje: Przy niespelnionym executable PnL threshold.
- Jak przetestowano: Test sprawdza `exit_blocked` oraz `truth_detail` z wymaganym progiem.
- Co pozostaje poza zakresem: Offline relabeling starych lifecycle artefaktow.

## Otwarte ryzyka / follow-up

- Rebuild/restart jest wymagany, aby kolejny runtime run faktycznie uzywal tej poprawki.
- Aktualny R34 pozostaje uruchomiony na binarce sprzed tej poprawki, zgodnie z dyspozycja pozostawienia go w tmux.
- Trzeba osobno zaprojektowac guard/regression check, ktory w CI lub smoke lifecycle wykryje kazdy `Target` z `final_pnl_pct < threshold` i kazdy `StopLoss` z `final_pnl_pct > -threshold`.
- Osobny temat: residual shadow simulation coverage drops (`Custom(6002)`, `Custom(2006)`, route materialization errors) nie sa naprawiane tym ADR.
