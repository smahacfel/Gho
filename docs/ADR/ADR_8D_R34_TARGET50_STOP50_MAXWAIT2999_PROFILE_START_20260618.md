# ADR-8D: R34 target50 stop50 maxwait2999 profile start

Status: DONE
Typ: runtime-config / shadow-run-start
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: /root/Gho / codex/gatekeeper-edge-policy-redesign-r1
Commit/PR: not committed
Zakres: R34 shadow-only rollout profile with 2999 ms observation window and +50% / -50% lifecycle thresholds
Dotkniete moduly/pliki:
- configs/rollout/ghost_brain_selector_dataset_sampler_r34_maxwait2999_fsc_off.toml
- configs/rollout/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1.toml
Powiazane runy/logi/raporty:
- tmux: r34-maxwait2999-target50-stop50
- PID: 2876428
- logs/rollout/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1/system.log
- logs/shadow_run/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1-buys.jsonl
- logs/shadow_run/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1/shadow_lifecycle.jsonl
- logs/shadow_run/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1/probe_transport.jsonl
- logs/shadow_run/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1/probe_shadow_lifecycle.jsonl
Poziom ryzyka: medium

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Upewnic sie, czy aktywne warunki symulacji TARGET/STOP sa ustawione na +50% / -50%, zatrzymac R33, przygotowac R34 na bazie R33 z identycznymi parametrami poza oknem obserwacji 2999 ms i progami lifecycle 50/50, uruchomic run w tmux i zostawic go dzialajacego.

Rzeczywisty przebieg:
Sprawdzono config R33 i aktywna sciezke runtime. R33 nie mial progow 50/50, tylko 30/30. Nie znaleziono aktywnego procesu R33 ani sesji tmux R33, wiec nie wysylano sygnalu kill. Utworzono osobny brain config R34 z `max_wait_time_ms = 2999` oraz osobny launcher config R34 z `live_exit_take_profit_pct = 0.50` i `live_exit_stop_loss_pct = 0.50`. R34 uruchomiono w tmux jako shadow-only.

Odchylenia od planu:
Pierwszy start tmux nie powstal, bo brakowalo katalogu logow. Utworzono wymagane katalogi runtime i ponowiono start. Podczas startu jeden topic NLN Program Streams (`solana.pump_fun.buy`) zakonczyl sie `Subscribe request failed`; `solana.pump_fun.buy_exact_sol_in` pracowal. Run pozostawiono dzialajacy, bo primary gRPC, decyzje i artefakty shadow/probe powstawaly.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia: Zmiana dotyczy Ghost runtime, shadow/live boundary, Gatekeeper decision evidence i artefaktow symulacyjnych.
Zakres uzycia: Sprawdzenie, ze zmiana pozostaje w konfiguracji rollout/shadow i nie zmienia Gatekeeper policy ani execution/send path.
Wynik: Zastosowano osobny profil R34 i zachowano shadow-only.
Ograniczenia: Skill nie potwierdza zdrowia zewnetrznego dostawcy NLN; to wymaga obserwacji logow runtime.

Nazwa: solana-pumpfun-architect
Powod uzycia: Start runu dotyka shadow simulation dla pump.fun oraz artefaktow lifecycle.
Zakres uzycia: Zachowano istniejaca sciezke symulacji i nie zmieniano builderow ani sendera.
Wynik: R34 zaczal emitowac BUY shadow lifecycle oraz counterfactual probe lifecycle.
Ograniczenia: Nie wykonywano zmian w Solana execution path.

Nazwa: rust-master
Powod uzycia: Weryfikacja przeplywu config -> PostBuyRuntime -> MonitoringEngine w kodzie Rust.
Zakres uzycia: Odczyt sciezki przekazania `live_exit_take_profit_pct` i `live_exit_stop_loss_pct`.
Wynik: Potwierdzono, ze konfiguracja jest przekazywana do `set_shadow_simple_exit_thresholds()`.
Ograniczenia: Nie kompilowano binarki, bo zmiana byla config-only i uzyto zbudowanej juz binarki.

## 3. Opis problemu - 3W2H

What:
Trzeba bylo przejsc na R34 z oknem obserwacji 2999 ms i lifecycle TARGET/STOP +50% / -50%.

Where:
Rollout config i brain config w `configs/rollout/`, uruchomienie przez `target/release/ghost-launcher`.

Why it matters:
Etykiety ekonomiczne i lifecycle exits musza odpowiadac aktualnemu kontraktowi badawczemu. Bledny prog 30/30 dalby inna populacje TARGET/STOP niz zamierzona.

How observed:
`rg` na konfiguracjach R33/R34 oraz w runtime code path. R33 mial `live_exit_take_profit_pct = 0.30` i `live_exit_stop_loss_pct = 0.30`; R34 ma `0.50` i `0.50`.

How many / scale:
R34 po starcie zaczal emitowac artefakty: BUY transport, BUY lifecycle, probe transport, probe lifecycle i probe skips. W pierwszym checku BUY transport mial 28 swiezych rows, w tym 25 `shadow_simulated`; probe transport mial 10 swiezych rows, w tym 9 `shadow_simulated`.

Evidence:
- `configs/rollout/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1.toml`: `live_exit_take_profit_pct = 0.50`, `live_exit_stop_loss_pct = 0.50`.
- `configs/rollout/ghost_brain_selector_dataset_sampler_r34_maxwait2999_fsc_off.toml`: `max_wait_time_ms = 2999`.
- `ghost-launcher/src/main.rs`: config przekazuje progi do `PostBuyRuntimeConfig`.
- `ghost-launcher/src/components/post_buy_runtime.rs`: progi trafiaja do `MonitoringEngine::set_shadow_simple_exit_thresholds()`.
- `shadow_lifecycle.jsonl`: pojawily sie `close_reason = Target` z `final_pnl_pct > 50` oraz `close_reason = StopLoss` z `final_pnl_pct < -50`.

## 4. Przyczyna zrodlowa

Root cause:
Nie dotyczy klasycznej awarii kodu. Poprzedni aktywny profil R33 nie reprezentowal nowego kontraktu 50/50, tylko starszy kontrakt 30/30.

Mechanizm bledu:
Gdyby R34 zostal uruchomiony bez jawnej zmiany `[trigger] live_exit_*`, lifecycle nadal klasyfikowalby target/stop wedlug 30/30.

Miejsce:
`[trigger] live_exit_take_profit_pct` i `live_exit_stop_loss_pct` w rollout configu.

Skutek:
Potencjalne niespojne etykiety TARGET/STOP wzgledem celu badawczego.

Dowod:
Porownanie R33 i R34 configow: R33 = 0.30/0.30, R34 = 0.50/0.50.

Odrzucone hipotezy:
- R33 byl juz aktywnie ustawiony na 50/50: odrzucone, config pokazywal 30/30.
- R33 nadal pracowal i wymagal kill: odrzucone, brak procesu i brak sesji tmux R33.
- Zmiana wymagala modyfikacji Gatekeeper policy: odrzucone, progi lifecycle sa w `[trigger]`, nie w Gatekeeper policy.

## 5. Strategia naprawy

Przyjeta strategia:
Utworzyc nowy, izolowany profil R34 przez skopiowanie R33 i minimalna zmiane: max wait do 2999 ms w brain configu oraz lifecycle thresholds do 50/50 w launcher configu.

Zakres ingerencji:
Tylko nowe configi R34 oraz uruchomienie shadow-only runu.

Czego nie zmieniano:
- Gatekeeper policy
- execution builder
- send path
- live execution
- scoring/model
- runtime Rust code
- DecisionLogger schema

Ryzyka:
- NLN topic `solana.pump_fun.buy` wystartowal, ale zakonczyl sie `Subscribe request failed`.
- FSC pozostaje fail-closed, bo `funding_lane_mode = "disabled"` zgodnie z profilem fsc_off.
- Pierwsza probka jest mala i nie sluzy do oceny edge, tylko do potwierdzenia runtime/artifact health.

Odrzucone alternatywy:
- Patchowanie aktywnego R33: odrzucone, nowy run powinien miec osobny namespace.
- Zmiana kodu runtime: odrzucone, progi sa juz config-driven.
- Zmiana Gatekeepera: poza zakresem.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `configs/rollout/ghost_brain_selector_dataset_sampler_r34_maxwait2999_fsc_off.toml`
- Co zmieniono: Utworzono profil R34 na bazie R33 z `max_wait_time_ms = 2999`.
- Dlaczego: R34 mial pracowac w oknie obserwacji 2999 ms.
- Efekt: Brain config R34 laduje identyczna semantyke jak R33 poza oknem obserwacji.

Zmiana 2:
- Plik/modul: `configs/rollout/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1.toml`
- Co zmieniono: Utworzono launcher config R34 z osobnym namespace/run_id/session_id, shadow-only, FSC off, `live_exit_take_profit_pct = 0.50`, `live_exit_stop_loss_pct = 0.50`.
- Dlaczego: R34 ma zbierac lifecycle zgodny z kontraktem +50% / -50%.
- Efekt: R34 wystartowal w tmux i zaczal emitowac BUY oraz counterfactual probe artefakty.

Zmiana 3:
- Plik/modul: runtime directories/logs
- Co zmieniono: Utworzono brakujace katalogi logow/datasets/data dla R34 i uruchomiono `ghost-launcher`.
- Dlaczego: Pierwszy start tmux nie powstal bez katalogu logow.
- Efekt: Sesja `r34-maxwait2999-target50-stop50` dziala, PID `2876428`.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Config parse | `python3 -c 'import tomllib; ...'` | Oba configi R34 parsuja sie poprawnie | PASS | TOML load bez bledu |
| Config fields | `rg -n "live_exit_take_profit_pct|live_exit_stop_loss_pct|max_wait_time_ms" configs/rollout/...r34...` | `max_wait_time_ms = 2999`, `live_exit_take_profit_pct = 0.50`, `live_exit_stop_loss_pct = 0.50` | PASS | Linie configu R34 |
| Code path | `rg -n "live_exit_take_profit_pct|set_shadow_simple_exit_thresholds" ghost-launcher/src/...` | Progi ida z configu do `PostBuyRuntimeConfig` i `MonitoringEngine` | PASS | `main.rs`, `post_buy_runtime.rs` |
| R33 stop check | `pgrep -af "ghost-launcher|shadow-burnin-v3-r33"` + `tmux ls` | Nie znaleziono aktywnego R33 | PASS | Brak procesu/sesji R33 |
| R34 start | `tmux new-session ... ghost-launcher --config ...r34...` | Proces dziala w tmux | PASS | tmux `r34-maxwait2999-target50-stop50`, PID `2876428` |
| Runtime artifacts | Python count fresh JSONL | BUY transport 28 fresh / 25 simulated; probe transport 10 fresh / 9 simulated | PASS | Artefakty pod `logs/shadow_run/...r34...` |
| Lifecycle threshold smoke | Python read `shadow_lifecycle.jsonl` | BUY lifecycle: Target=9, StopLoss=4, TimeStop=8; Target rows maja `final_pnl_pct > 50`, StopLoss rows maja `final_pnl_pct < -50` | PASS | `shadow_lifecycle.jsonl` |
| NLN startup | `rg -n "NLN Program Streams" system.log` | `buy_exact_sol_in` dostal first message; `solana.pump_fun.buy` zakonczyl sie `Subscribe request failed` | WARN | `system.log` lines 163, 312, 313, 799 |
| Disk | `df -h /root/Gho` | 41G wolne na `/` | PASS | `/dev/sda1 150G 103G 41G 72% /` |

Wniosek walidacyjny:
R34 wystartowal w oczekiwanym profilu shadow-only, z oknem 2999 ms i progami lifecycle +50% / -50%. Progi sa aktywne w runtime, bo lifecycle zamyka pozycje jako `Target` powyzej +50% i `StopLoss` ponizej -50%. Counterfactual probe rowniez emituje artefakty, wiec run nie zbiera wylacznie BUY lifecycle.

Ograniczenia walidacji:
Probka startowa jest mala. Jeden topic NLN Program Streams nie utrzymal subskrypcji. Nie wykonano build/test, bo nie zmieniano kodu Rust.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: config isolation
- Co zabezpiecza: R34 nie nadpisuje R33 ani innych profili.
- Kiedy sie aktywuje: Przy kazdym uruchomieniu R34 uzywany jest osobny namespace/run_id/session_id.
- Jak przetestowano: Sprawdzono sciezki i ID w configu R34 oraz artefakty pod osobnym katalogiem R34.
- Co pozostaje poza zakresem: Nie wymusza automatycznej walidacji wszystkich przyszlych profili.

Guardrail 2:
- Typ: runtime proof
- Co zabezpiecza: Kontrakt 50/50 jest potwierdzony przez realny lifecycle, a nie tylko przez config.
- Kiedy sie aktywuje: Po powstaniu pierwszych `position_closed` rows.
- Jak przetestowano: Odczytano `close_reason` i `final_pnl_pct` z R34 lifecycle.
- Co pozostaje poza zakresem: Nie ocenia edge ani jakosci Gatekeepera.

## Otwarte ryzyka / follow-up

- Zbadac, dlaczego `solana.pump_fun.buy` w NLN Program Streams zakonczyl sie `Subscribe request failed` przy starcie R34.
- Kontynuowac monitoring R34 coverage na wiekszej probce, szczegolnie BUY `shadow_simulated` oraz probe `shadow_simulated` vs `probe_skips`.
- Jesli NLN `solana.pump_fun.buy` jest wymagany dla pelnego coverage, rozwazyc restart albo oddzielna diagnoze dostawcy/tematu, ale nie mieszac tego z Gatekeeper policy.
