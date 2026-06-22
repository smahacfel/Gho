# ADR-8D: R38 threshold-probe maxwait31100 profile start

Status: IMPLEMENTED / RUNTIME_SMOKE_RUNNING
Typ: ADR-8D / rollout profile / shadow-only counterfactual probe
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `main`
Commit/PR: not committed at report time
Zakres: R38 shadow-only threshold-probe launch profile with `max_wait_time_ms = 31100`
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`
- `configs/rollout/shadow-burnin-v3-r38-threshold-probe-target50-stop50-fsc-off-r1.toml`
- `docs/ADR/ADR_8D_R38_THRESHOLD_PROBE_PROFILE_START_20260618.md`

Powiazane runy/logi/raporty:
- R38 tmux session: `r38-threshold-probe-maxwait31100`
- R38 rollout config: `configs/rollout/shadow-burnin-v3-r38-threshold-probe-target50-stop50-fsc-off-r1.toml`
- R38 brain config: `configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`
- R38 runtime log: `reports/selector/shadow-burnin-v3-r38-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260618T185437Z/runtime.log`
- R38 decision logs: `logs/rollout/shadow-burnin-v3-r38-threshold-probe-target50-stop50-fsc-off-r1/decisions/`
- R38 probe logs: `logs/shadow_run/shadow-burnin-v3-r38-threshold-probe-target50-stop50-fsc-off-r1/`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Przygotowac R38 jako nowy shadow-only threshold-probe run na brain configu `ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`, uruchomic go w tmux i sprawdzic, czy runtime zyje przez pierwsze dwie minuty.

Rzeczywisty przebieg:
- Potwierdzono brak aktywnego procesu `ghost-launcher` poza nowym R38.
- Potwierdzono, ze `target/release/ghost-launcher` jest nowszy niz patch w `ghost-launcher/src/oracle_runtime.rs`, wiec rebuild nie byl wymagany.
- Utworzono rollout profile R38 z osobnym namespace, sciezkami logow/datasetow i wskazaniem na brain config R38.
- Poprawiono naglowek brain configu z mylacego R37/3789 na R38/31100.
- Uruchomiono R38 w tmux.
- Po okolo dwoch minutach runtime nadal emitowal decyzje i probe artifacts.

Odchylenia od planu:
Nie wykonano rebuilda release binary, bo aktualna binarka byla juz nowsza niz zmienione zrodla. `extended_window_ms = 3789` zostawiono bez zmiany, bo dyspozycja dotyczyla profilu `maxwait31100`, a nie zmiany tego pola.

## 2. Wykorzystane skills/sub-agenci

Nazwa:
`ghost-execution`

Powod uzycia:
Start nowego shadow runtime dotyka DecisionLogger, shadow/probe artifacts, active-vs-shadow boundary oraz P37 counterfactual probe.

Zakres uzycia:
Utrzymanie runa jako shadow-only data collection/probe, bez zmiany BUY/REJECT/TIMEOUT policy, execution ani send path.

Wynik:
R38 zostal uruchomiony jako osobny namespace z FSC disabled i P37 shadow probe enabled.

Ograniczenia:
Smoke dwuminutowy nie jest pelnym coverage proof ani koncowym R2/business-label datasetem.

Nazwa:
Config Rollout Safety Reviewer

Powod uzycia:
Zadanie tworzy nowy rollout config i wykorzystuje brain config z wartoscia `max_wait_time_ms = 31100`.

Zakres uzycia:
Sprawdzenie, ze zmiana pozostaje w profilu rollout/shadow-only, bez ukrytej zmiany live execution.

Wynik:
TOML obu configow parsuje sie poprawnie; `funding_lane_mode = "disabled"` i `entry_mode = "shadow_only"` pozostaja jawne.

Ograniczenia:
Nie oceniano statystycznej jakosci progow; to wymaga pozniejszej analizy artifacts.

## 3. Opis problemu - 3W2H

What:
Potrzebny byl nowy run R38 na profilu z `max_wait_time_ms = 31100`, aby sprawdzic runtime po naprawie shadow-probe route materialization i zbierac nowe artefakty z osobnego namespace.

Where:
`configs/rollout/` oraz runtime `ghost-launcher --config`.

Why it matters:
Bez osobnego R38 namespace nowe artefakty moglyby mieszac sie z R37/R35. Bez rollout profile `ghost-launcher` nie powinien byc uruchamiany bezposrednio na samym brain configu.

How observed:
Na dysku istnial brain config R38, ale nie istnial odpowiadajacy mu rollout profile `shadow-burnin-v3-r38-threshold-probe-target50-stop50-fsc-off-r1.toml`.

How many / scale:
Po dwuminutowym smoke R38 mial 107 decision rows, 59 probe selections, 53 probe transports, 53 probe entries i 76 lifecycle rows.

Evidence:
`tmux ls` pokazal sesje `r38-threshold-probe-maxwait31100`, a `pgrep` aktywny proces `/root/Gho/target/release/ghost-launcher --config ...r38...toml`.

## 4. Przyczyna zrodlowa

Root cause:
R38 brain config zostal przygotowany osobno, ale brakowalo rollout wrapper configu wskazujacego wszystkie runtime paths, namespaces i log destinations.

Mechanizm bledu:
`ghost-launcher --config` w tym workflow startuje rollout profile, ktory dopiero wskazuje `ghost_brain_config_path`. Sam brain config nie materializuje pelnego profilu runtime/logging/probe paths.

Miejsce:
`configs/rollout/`.

Skutek:
Przed zmiana R38 nie mial kompletnego, izolowanego runtime profilu.

Dowod:
`ls configs/rollout/*r38*` pokazywal tylko brain config R38, bez `shadow-burnin-v3-r38...toml`.

Odrzucone hipotezy:
- Rebuild wymagany przed startem: odrzucone, binarka release byla nowsza niz zmienione zrodla.
- Uruchomienie brain configu bez rollout wrappera: odrzucone, bo nie daje pelnej kontroli namespace/log paths.

## 5. Strategia naprawy

Przyjeta strategia:
Skopiowac semantyke R37 rollout profile do osobnego R38 namespace, zmieniajac tylko identyfikatory runa, sciezki artefaktow i `ghost_brain_config_path`.

Zakres ingerencji:
- Nowy R38 rollout profile.
- Korekta komentarza w R38 brain configu.
- Start shadow-only runtime w tmux.

Czego nie zmieniano:
- Gatekeeper policy code.
- Execution/send path.
- DirectBuyBuilder.
- FSC enablement.
- Runtime BUY/REJECT/TIMEOUT semantics.
- Progi po starcie runa.

Ryzyka:
- Dwuminutowy smoke nie przesadza o docelowym coverage.
- W logu pojawily sie `ERROR` wpisy post-buy `PANIC SELL` z guardian shadow lane; nie byly to procesowe paniki Rust ani stream-limit failures.

Odrzucone alternatywy:
- Start na brain configu bez rollout wrappera: odrzucone.
- Rebuild release binary mimo swiezego targetu: odrzucone jako niepotrzebne dla tego startu.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `configs/rollout/shadow-burnin-v3-r38-threshold-probe-target50-stop50-fsc-off-r1.toml`
- Co zmieniono: dodano osobny rollout profile R38 z namespace, log paths, probe paths i `ghost_brain_config_path` na R38 brain config.
- Dlaczego: `ghost-launcher` potrzebuje kompletnego runtime profile, nie tylko brain configu.
- Efekt: R38 uruchamia sie w izolowanym namespace.

Zmiana 2:
- Plik/modul: `configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`
- Co zmieniono: poprawiono naglowek komentarza z R37/3789 na R38/31100.
- Dlaczego: stary komentarz byl mylacy dla audytu configu.
- Efekt: opis configu zgadza sie z nazwa i wartoscia `max_wait_time_ms = 31100`.

Zmiana 3:
- Plik/modul: runtime/tmux
- Co zmieniono: uruchomiono `/root/Gho/target/release/ghost-launcher --config /root/Gho/configs/rollout/shadow-burnin-v3-r38-threshold-probe-target50-stop50-fsc-off-r1.toml` w tmux `r38-threshold-probe-maxwait31100`.
- Dlaczego: dyspozycja wymagla startu R38 i dwuminutowego health-checku.
- Efekt: R38 pracuje w tle i emituje decyzje oraz probe artifacts.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| TOML parse | `python3 -c 'import tomllib, pathlib; ...'` | oba configi parsuja sie poprawnie | PASS | `toml_ok` |
| Binary freshness | `stat target/release/ghost-launcher ghost-launcher/src/oracle_runtime.rs` | release binary `2026-06-18 18:37:38`, source `18:22:46` | PASS | build nie wymagany |
| Runtime start | `tmux new-session ... ghost-launcher --config ...r38...toml` | proces `ghost-launcher` aktywny | PASS | PID `3359096` |
| Smoke artifacts | first 2 minutes | 107 decision rows, 59 selections, 53 transports, 53 entries, 76 lifecycle rows | PASS | `logs/rollout/...r38...`, `logs/shadow_run/...r38...` |
| Guard negative case | log scan | brak `ResourceExhausted`, stream-limit rejection, Rust panic, `Custom(2006)` w smoke scan | PASS | `rg` scan runtime/oracle logs |

Wniosek walidacyjny:
R38 zostal poprawnie wystartowany i przez pierwsze dwie minuty emitowal decyzje oraz probe/lifecycle artifacts. Run pozostaje shadow-only i nie jest BUY-selector validation.

Ograniczenia walidacji:
Dwuminutowy smoke nie wystarcza do oceny finalnego simulation coverage, FSC coverage, R2 labels ani business outcome.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: namespace/path isolation
- Co zabezpiecza: brak mieszania R38 artifacts z R37/R35
- Kiedy sie aktywuje: wszystkie R38 output paths zawieraja `shadow-burnin-v3-r38-threshold-probe-target50-stop50-fsc-off-r1`
- Jak przetestowano: smoke artifacts powstaly w katalogach R38
- Co pozostaje poza zakresem: pozniejszy post-run labeling

Guardrail 2:
- Typ: shadow/live boundary
- Co zabezpiecza: R38 nie wlacza live execution ani FSC
- Kiedy sie aktywuje: config ma `entry_mode = "shadow_only"`, `execution_mode = "shadow"`, `funding_lane_mode = "disabled"`
- Jak przetestowano: TOML parse i config inspection
- Co pozostaje poza zakresem: dlugookresowa jakosc coverage

## Otwarte ryzyka / follow-up

- Po dluzszym czasie trzeba policzyc pelne simulation coverage i skip breakdown.
- Trzeba odroznic expected shadow guardian `PANIC SELL` logi od realnych runtime failures w monitoringu.
- Po zamknieciu runa R38 trzeba zbudowac finalne selector/R2/business-label artifacts, jezeli bedzie uzywany do analizy.
