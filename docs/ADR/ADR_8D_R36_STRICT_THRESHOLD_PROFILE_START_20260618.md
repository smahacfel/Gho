# ADR-8D: R36 strict threshold profile start

Status: IMPLEMENTED / R36_RUNNING_WITH_LIFECYCLE_PROOF
Typ: ADR-8D / rollout profile / selector lifecycle run / strict-threshold probe
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: /root/Gho / backup-vps
Commit/PR: local working tree; not committed at report time
Zakres: R36 runtime profile derived from R35 strict threshold semantics, launched through selector lifecycle launcher
Dotkniete moduly/pliki:
- configs/rollout/ghost_brain_selector_dataset_sampler_r36_threshold_probe_maxwait3789_fsc_off.toml
- configs/rollout/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1.toml
- docs/ADR/ADR_8D_R36_STRICT_THRESHOLD_PROFILE_START_20260618.md
Powiazane runy/logi/raporty:
- reports/selector/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260618T103640Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json
- reports/selector/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260618T103640Z/RUN_LIFECYCLE_LAUNCHER_REPORT.md
- reports/selector/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260618T103640Z/runtime.log
- logs/rollout/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1/decisions/
- logs/shadow_run/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1-buys.jsonl
- logs/shadow_run/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1/shadow_lifecycle.jsonl
- logs/shadow_run/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1/probe_selection.jsonl
- logs/shadow_run/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1/probe_shadow_lifecycle.jsonl
Poziom ryzyka: MEDIUM

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Utworzyc R36 profile analogiczne do R35 po naprawie strict threshold hard gate, uruchomic zgodnie z procedura selector lifecycle launcher i zostawic runtime w tmux. R36 mial zachowac progi R35:
min_tx_count=5, min_unique_signers=2, min_buy_count=2, max_wait_time_ms=3789, signer_cross_pool_velocity < 0.9737,
jito_tip_intensity < 0.3485, flipper_presence_ratio >= 0.19, burst_ratio >= 0.27.

Rzeczywisty przebieg:
Skopiowano R35 rollout config i Ghost Brain config do R36, podmieniajac scope, sciezki i identyfikatory runu.
Potwierdzono wartosci progow, shadow-only execution, target/stop 50% oraz wlaczony p37 shadow probe dla BUY/REJECT/TIMEOUT/PENDING.
Zbudowano `ghost-launcher` w release mode po naprawie R35 strict gate, a R36 uruchomiono przez `scripts/start_selector_lifecycle_run.py`.
Launcher zakonczyl `PASS` z claimem `SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF` i zostawil runtime w tmux session `r36-threshold-probe-target50-stop50`.

Odchylenia od planu:
- Pierwszy dry-run bez `--skip-static-tests` zawisl na `guard_restore_shadow_lifecycle.py --skip-runtime`; proces dry-run zostal zatrzymany, bez uruchamiania runtime.
- Wlasciwy start wykonano przez ten sam launcher z `--skip-static-tests`; static guard w raporcie R36 mimo tego zakonczyl `PASS` z `--skip-tests --skip-runtime`.
- Nie commitowano configow ani raportow.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia:
Run dotyka aktywnej sciezki Gatekeeper V2/V2.5, shadow execution, DecisionLogger evidence i lifecycle proof.
Zakres uzycia:
Zweryfikowano, ze R36 nie zmienia execution/send path, pracuje w shadow-only i zachowuje osobny p37 shadow probe output.
Wynik:
R36 zostal uruchomiony jako selector lifecycle run z proof, a nie manualny tmux start.
Ograniczenia:
Skill nie zastepuje dlugiego runtime burn-in; potwierdza tylko poprawny start i pierwsze lifecycle proof.

Nazwa: rust-master
Powod uzycia:
Przed startem potrzebny byl release build binarki po zmianach Rust naprawiajacych strict threshold gate.
Zakres uzycia:
Uruchomiono `cargo build -p ghost-launcher --release`.
Wynik:
Build zakonczyl sie sukcesem; runtime uzywa `/root/Gho/target/release/ghost-launcher`.
Ograniczenia:
Build emitowal istniejace warningi poza zakresem R36.

Nazwa: Config Rollout Safety Reviewer
Powod uzycia:
Zmiana dotyczy configow rollout/Ghost Brain, progow decyzyjnych i shadow/live boundary.
Zakres uzycia:
Sprawdzono scope, sciezki logow, `funding_lane_mode=disabled`, `entry_mode=shadow_only`, `execution_mode=shadow`,
oraz threshold contract R36.
Wynik:
Config contract i scope contract w launcher report maja status `PASS`.
Ograniczenia:
Nie przeprowadzano semantycznego audytu wszystkich historycznych progow w duzym Ghost Brain configu.

## 3. Opis problemu - 3W2H

What:
Po zatrzymaniu i naprawie wadliwego R35 potrzebny byl nowy run R36, ktory realnie egzekwuje analogiczny zestaw progow jako strict hard gate
i jednoczesnie zbiera klasyczne BUY lifecycle oraz probe lifecycle dla pozostalych decyzji.

Where:
Rollout profile:
- `configs/rollout/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1.toml`
- `configs/rollout/ghost_brain_selector_dataset_sampler_r36_threshold_probe_maxwait3789_fsc_off.toml`

Why it matters:
R35 sprzed naprawy nie mogl byc traktowany jako poprawny test progow. R36 ma dostarczyc nowa probke, w ktorej niespelnienie dowolnego
z wymaganych progow skutkuje strict threshold reject, a shadow probe nadal pozwala symulowac nie tylko BUY.

How observed:
R36 launcher report wskazuje `SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF`, `event_canary PASS`, `lifecycle_canary PASS`,
`config_contract PASS`, `scope_contract PASS`, `preflight PASS`.

How many / scale:
Na pierwszym checku po starcie byly juz emitowane:
- 94 rows w obu `gatekeeper_v2_decisions.jsonl`
- 11 rows w `shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1-buys.jsonl`
- 21 rows w klasycznym `shadow_lifecycle.jsonl`
- 29 rows w `probe_selection.jsonl`
- 34 rows w `probe_shadow_lifecycle.jsonl`

Evidence:
- `RUN_LIFECYCLE_LAUNCHER_REPORT.md`: status `PASS`, run_state `RUN_LEFT_RUNNING_AFTER_LIFECYCLE_PROOF`
- `RUN_LIFECYCLE_LAUNCHER_REPORT.json`: storage/config/scope/static/preflight/event/lifecycle PASS
- `tmux ls`: aktywna sesja `r36-threshold-probe-target50-stop50`

## 4. Przyczyna zrodlowa

Root cause:
R35 wymagal uniewaznienia po wykryciu niepelnej egzekucji progow; potrzebny byl nowy, czysty scope R36 na naprawionej binarce.

Mechanizm bledu:
Nie dotyczy nowego runtime bledu R36. R36 jest odpowiedzia operacyjna na poprzedni blad R35: stare dane R35 nie mogly byc dalej uzywane jako threshold probe.

Miejsce:
Config/rollout layer oraz selector lifecycle launcher.

Skutek:
Powstal nowy izolowany scope R36 zamiast nadpisywania lub reanimowania R35.

Dowod:
R36 ma osobne sciezki `logs/`, `datasets/events/`, `data/rollout/` i osobny report folder.

Odrzucone hipotezy:
- "Wystarczy wznowic R35": odrzucone, bo R35 byl invalid jako pierwotny run.
- "Manualny tmux start wystarczy": odrzucone, bo runbook wymaga selector lifecycle launcher proof.
- "Trzeba zmienic Gatekeeper policy": odrzucone, R36 bazuje na juz naprawionej logice i zmienia tylko profil/scope.

## 5. Strategia naprawy

Przyjeta strategia:
Utworzyc nowy R36 scope przez mechaniczne skopiowanie R35 configow po naprawie, potwierdzic progi i sciezki, zbudowac release binary,
a nastepnie wystartowac launcherem lifecycle z proof gates.

Zakres ingerencji:
- Dwa nowe pliki configu R36.
- Jeden ADR-8D.
- Brak zmian kodu w tym kroku.

Czego nie zmieniano:
- Gatekeeper policy.
- Execution.
- Send path.
- Funding lane / FSC, ktore pozostaje disabled.
- Runtime thresholds po starcie.
- Selector/model/scoring.
- Istniejace artefakty R35/R34.

Ryzyka:
- Dysk pozostaje wysoki: ok. 14 GB wolne na `/` przy checku po starcie.
- R36 strict gate moze mocno ograniczyc BUY count; to jest oczekiwany efekt testu progow.
- `ERROR` logi z WHF/PANIC SELL w shadow post-buy moga wygladac alarmujaco, ale nie byly markerami `ResourceExhausted` ani stream limit.

Odrzucone alternatywy:
- Zmieniac progi w trakcie runu: odrzucone.
- Wlaczyc FSC/full-chain: odrzucone, R36 ma byc analogiczny do R35 i FSC off.
- Uruchomic bez launcher proof: odrzucone.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `configs/rollout/ghost_brain_selector_dataset_sampler_r36_threshold_probe_maxwait3789_fsc_off.toml`
- Co zmieniono: utworzono R36 Ghost Brain profile z progami R35 i `strict_metric_threshold_gate_enabled=true`.
- Dlaczego: R36 ma testowac ten sam hard-gate contract na czystym scope.
- Efekt: profil ma `mode=long`, `max_wait_time_ms=3789`, count gates i wymagane min/max metryk antysybil/anticabal.

Zmiana 2:
- Plik/modul: `configs/rollout/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1.toml`
- Co zmieniono: utworzono rollout profile R36 z osobnymi sciezkami logs/datasets/data, shadow-only execution i p37 shadow probe.
- Dlaczego: run musi byc izolowany od R35 i zbierac klasyczne BUY lifecycle oraz probe lifecycle dla pozostalych decyzji.
- Efekt: R36 pisze do osobnych artefaktow i uzywa R36 Ghost Brain configu.

Zmiana 3:
- Plik/modul: release binary
- Co zmieniono: uruchomiono `cargo build -p ghost-launcher --release`.
- Dlaczego: runtime mial wystartowac na binarce zawierajacej strict gate fix.
- Efekt: `/root/Gho/target/release/ghost-launcher` ma mtime `2026-06-18T10:36:22.592996+00:00`.

Zmiana 4:
- Plik/modul: runtime launch
- Co zmieniono: uruchomiono `scripts/start_selector_lifecycle_run.py` dla R36 i zostawiono runtime w tmux.
- Dlaczego: selector lifecycle-capable runs musza miec launcher proof.
- Efekt: launcher report ma `PASS`, a runtime dziala w `r36-threshold-probe-target50-stop50`.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Config grep | `rg -n "max_wait_time_ms|min_tx_count|..." configs/rollout/*r36*` | progi R35 obecne w R36 | PASS | `max_wait_time_ms=3789`, `min_tx_count=5`, `strict_metric_threshold_gate_enabled=true` |
| Diff hygiene | `git diff --check` | brak bledow whitespace | PASS | command exit 0 przed startem |
| Build | `cargo build -p ghost-launcher --release` | build zakonczony sukcesem; warnings only | PASS | release binary mtime `2026-06-18T10:36:22.592996+00:00` |
| Launcher proof | `scripts/start_selector_lifecycle_run.py ...` | `SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF` | PASS | `RUN_LIFECYCLE_LAUNCHER_REPORT.json` |
| Event canary | launcher event canary | status `PASS` | PASS | `event_canary/RUN_LIFECYCLE_CANARY_PROOF.json` |
| Lifecycle canary | launcher lifecycle canary | status `PASS` | PASS | `lifecycle_canary/RUN_LIFECYCLE_CANARY_PROOF.json` |
| Runtime state | `tmux ls` / `pgrep -af ghost-launcher` | R36 running in tmux | PASS | `r36-threshold-probe-target50-stop50`, pid `3096768` at check |
| Artifact emission | `wc -l` selected R36 jsonl files | BUY and probe artifacts present | PASS | `shadow_lifecycle`, `probe_selection`, `probe_shadow_lifecycle` non-empty |

Wniosek walidacyjny:
R36 zostal poprawnie przygotowany i uruchomiony jako lifecycle-proven selector run. Na etapie startowym potwierdzono ingest, decision rows,
klasyczna shadow lifecycle dla BUY oraz probe lifecycle dla pozostalych decyzji.

Ograniczenia walidacji:
- Nie wykonano dlugiej oceny precision/coverage; run ma pracowac dalej.
- Nie budowano jeszcze finalnego datasetu/labelingu R36.
- Dry-run bez `--skip-static-tests` zawisl i zostal przerwany przed startem runtime; wlasciwy start ma jednak launcher PASS.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: isolated rollout scope
- Co zabezpiecza: oddzielenie R36 od niewaznego R35
- Kiedy sie aktywuje: wszystkie sciezki R36 maja dedykowany scope
- Jak przetestowano: config grep i launcher scope contract PASS
- Co pozostaje poza zakresem: pozniejszy cleanup starych artefaktow

Guardrail 2:
- Typ: lifecycle launcher proof
- Co zabezpiecza: manualny start bez proof nie jest traktowany jako poprawny selector lifecycle run
- Kiedy sie aktywuje: start przez `scripts/start_selector_lifecycle_run.py`
- Jak przetestowano: `RUN_LIFECYCLE_LAUNCHER_REPORT` ma status PASS
- Co pozostaje poza zakresem: dlugoterminowy runtime health

Guardrail 3:
- Typ: shadow/live boundary guard
- Co zabezpiecza: brak przypadkowego live/send path
- Kiedy sie aktywuje: config `entry_mode=shadow_only`, `execution_mode=shadow`
- Jak przetestowano: grep configu i launcher config contract PASS
- Co pozostaje poza zakresem: przyszle zmiany configu po starcie

## Otwarte ryzyka / follow-up

- Monitorowac, czy strict threshold reject rzeczywiscie pojawia sie w verdict/reason breakdown po wiekszej probce.
- Monitorowac disk free; przy pierwszym checku po starcie wolne bylo ok. 14 GB.
- Po zamknieciu R36 zbudowac finalny all-decision universe/labels i osobno policzyc BUY lifecycle oraz p37 probe simulation coverage.
- Nie commitowac raportow/logow/datasetow; commitowalne sa tylko configi i ADR, jesli uzytkownik pozniej to zleci.
