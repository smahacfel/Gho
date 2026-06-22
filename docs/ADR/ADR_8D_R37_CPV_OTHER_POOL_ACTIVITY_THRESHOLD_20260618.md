# ADR-8D: R37 CPV other-pool activity threshold

Status: IMPLEMENTED / R37_RUNNING_WITH_PROBE / BUY_LIFECYCLE_GUARD_NO_BUY
Typ: ADR-8D / runtime threshold extension / Gatekeeper V2 strict hard gate / rollout profile
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: /root/Gho / backup-vps
Commit/PR: local working tree; not committed at report time
Zakres: R36 audit and shutdown, CPV other-pool activity materialization, strict threshold enforcement, R37 profile launch
Dotkniete moduly/pliki:
- ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs
- ghost-core/src/tx_intelligence/types.rs
- ghost-launcher/src/session/observation.rs
- ghost-brain/src/config/ghost_brain_config.rs
- ghost-brain/src/oracle/decision_logger.rs
- ghost-launcher/src/components/gatekeeper.rs
- ghost-launcher/src/components/gatekeeper_policy.rs
- ghost-launcher/tests/gatekeeper_policy_tests.rs
- ghost-launcher/tests/gatekeeper_v2_pipeline_integration.rs
- ghost-launcher/tests/full_pipeline_integration.rs
- configs/rollout/ghost_brain_selector_dataset_sampler_r37_threshold_probe_maxwait3789_fsc_off.toml
- configs/rollout/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1.toml
- docs/ADR/ADR_8D_R37_CPV_OTHER_POOL_ACTIVITY_THRESHOLD_20260618.md
Powiazane runy/logi/raporty:
- reports/selector/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1/R36_RUNTIME_AUDIT_SUMMARY.md
- reports/selector/shadow-burnin-v3-r36-threshold-probe-target50-stop50-fsc-off-r1/R36_RUNTIME_AUDIT_SUMMARY.json
- reports/selector/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260618T121843Z/event_canary/RUN_LIFECYCLE_CANARY_PROOF.md
- reports/selector/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260618T121843Z/lifecycle_canary/RUN_LIFECYCLE_CANARY_PROOF.md
- reports/selector/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260618T121843Z/runtime.log
- logs/rollout/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/decisions/
- logs/shadow_run/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/probe_selection.jsonl
- logs/shadow_run/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/probe_skips.jsonl
- logs/shadow_run/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/probe_transport.jsonl
- logs/shadow_run/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/probe_shadow_entries.jsonl
- logs/shadow_run/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/probe_shadow_lifecycle.jsonl
Poziom ryzyka: MEDIUM

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Najpierw wykonac audyt R36, w tym coverage klasycznej BUY shadow simulation i all-decision probe simulation. Nastepnie zatrzymac R36,
utworzyc R37 profile na tych samych progach co R36, zmieniajac tylko aktywny prog `jito_tip_intensity` na `0.33617` oraz dodajac
aktywny hard gate `cpv_other_pool_activity > 8.5`. Jezeli w kodzie nie istnieje `min_cpv_other_pool_activity`, dodac go bez naruszania
SSOT, DecisionLogger i backward compatibility. Po walidacji zbudowac projekt i uruchomic R37 w tmux.

Rzeczywisty przebieg:
R36 zostal zanalityzowany i zatrzymany. Potwierdzono, ze R36 mial 1330 decyzji w obu planach, 196 klasycznych BUY, 196 klasycznych
shadow transport rows i 164 pelne klasyczne sukcesy `shadow_simulated`. All-decision probe mial 230 selection rows, 199 transport rows
oraz 159 zamknietych probe lifecycle pozycji. FSC pozostawal disabled.

Nastepnie dodano nowe pole `cpv_other_pool_activity` do CPV computation i materializacji `SybilResistanceFeatures`, dodano
`min_cpv_other_pool_activity` do configu Gatekeeper V2 z `#[serde(default)]`, dopisano pole do buy loga i selector feature map,
oraz wlaczono fail-closed hard gate w `strict_metric_threshold_failure_from_assessment`. R37 profile zostal utworzony i uruchomiony
przez selector lifecycle launcher w tmux.

Odchylenia od planu:
- Standardowy lifecycle canary dla klasycznego BUY-path nie przeszedl dla R37, bo w pierwszym proof window nie wystapil zaden klasyczny BUY.
- Jednoczesnie all-decision probe dla R37 emituje selection/transport/entries/lifecycle, czyli sciezka istotna dla probe simulation dziala.
- FSC pozostaje disabled tak jak w R36/R37 config; coverage FSC nie dotyczy tego profilu.
- Nie commitowano zmian ani artefaktow.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia:
Zmiana dotyka aktywnego Gatekeeper V2/V2.5, SSOT materializacji, shadow simulation i DecisionLogger evidence.
Zakres uzycia:
Sprawdzono, ze nowe pole przechodzi przez CPV computation -> `MaterializedFeatureSet`/`SybilResistanceFeatures` -> Gatekeeper policy
-> DecisionLogger, bez odczytow live state w policy.
Wynik:
Nowy threshold jest config-driven, audytowalny w logach i dziala jako strict hard gate tylko po ustawieniu wartosci dodatniej.
Ograniczenia:
Skill nie ocenia statystycznej jakosci progu `8.5`; ten run jest eksperymentem runtime/probe, nie potwierdzeniem edge.

Nazwa: rust-master
Powod uzycia:
Zmiana dotyczy Rust hot path w CPV computation, materializacji i policy evaluation.
Zakres uzycia:
Zachowano proste liczenie w istniejacej strukturze CPV, bez nowych async blokad i bez zewnetrznych odczytow w policy.
Wynik:
Kod sformatowany `rustfmt`, testy targetowane i release build przeszly.
Ograniczenia:
Nie robiono mikrobenchmarku; koszt CPV pozostaje w ramach dotychczasowego przebiegu CPV computation.

Nazwa: Config Rollout Safety Reviewer
Powod uzycia:
Dodano nowy config threshold zmieniajacy BUY/REJECT behavior.
Zakres uzycia:
Sprawdzono `#[serde(default)]`, default `0.0` jako inactive/backward-compatible, oraz R37 config delta.
Wynik:
Stare configi powinny sie ladowac bez nowego pola, a nowy hard gate aktywuje sie tylko dla `min_cpv_other_pool_activity > 0.0`.
Ograniczenia:
Nie przeprowadzono migracji historycznych configow, bo nie byla wymagana.

Nazwa: Gatekeeper Policy Auditor
Powod uzycia:
Nowy threshold jest hard gate i musi byc deterministyczny, reason-coded i fail-closed przy braku feature.
Zakres uzycia:
Zweryfikowano, ze brak albo niefinitywna wartosc `cpv_other_pool_activity` przy aktywnym progu daje hard fail, a wartosc `<= threshold`
rowniez daje hard fail.
Wynik:
Niespelnienie progu `cpv_other_pool_activity > 8.5` skutkuje `REJECT_HARD_FAIL` / strict metric threshold path.
Ograniczenia:
Nie zmieniano innych warstw Gatekeepera ani modulow V2.5.

## 3. Opis problemu - 3W2H

What:
R36 dzialal na zestawie progow strict threshold, ale kolejny run R37 mial dodatkowo wymusic prog dla CPV other-pool activity, ktorego
nie bylo dotad jako osobnej, configowalnej metryki decyzyjnej.

Where:
Nowy sygnal powstaje w:
- `ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs`

Nastepnie przechodzi przez:
- `ghost-core/src/tx_intelligence/types.rs`
- `ghost-launcher/src/session/observation.rs`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-brain/src/oracle/decision_logger.rs`

Aktywny rollout:
- `configs/rollout/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1.toml`

Why it matters:
Uzytkownik chcial, aby R37 odrzucal kandydata, jezeli nie spelni choc jednego z progow, w tym nowego warunku
`cpv_other_pool_activity > 8.5`. Bez jawnego field/config/policy path nie dalo sie tego egzekwowac audytowalnie.

How observed:
Przed zmiana istnialy inne CPV/sybil metryki, ale nie bylo aktywnego `min_cpv_other_pool_activity` w configu i strict policy.
Po zmianie decyzje R37 zawieraja `cpv_other_pool_activity` i `min_cpv_other_pool_activity`; na pierwszym snapshotcie R37 47/123 legacy
decision rows mialo pole CPV obecne, a legacy reason/fingerprint wskazywal 14 CPV fail mentions.

How many / scale:
R36 audit:
- 1330 legacy/v25 decisions
- 196 klasycznych BUY
- 164/196 klasyczne transport rows bez error_class
- 162/196 klasyczne `position_closed`
- 230 all-decision probe selection rows
- 199 all-decision probe transport rows
- 159 all-decision probe `position_closed`

Pierwszy R37 snapshot:
- 123 legacy/v25 decisions
- 22 probe selection rows
- 102 probe skip rows
- 20 probe transport rows
- 20 probe shadow entries
- 14 probe lifecycle rows

Evidence:
- `R36_RUNTIME_AUDIT_SUMMARY.md`
- R37 `event_canary/RUN_LIFECYCLE_CANARY_PROOF.md`: event canary PASS
- R37 `lifecycle_canary/RUN_LIFECYCLE_CANARY_PROOF.md`: BUY lifecycle proof FAIL due no classic BUY lifecycle in proof window
- R37 probe artifacts under `logs/shadow_run/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/`

## 4. Przyczyna zrodlowa

Root cause:
Metryka `cpv_other_pool_activity` nie byla dotad pelnoprawnym SSOT field materializowanym do decyzji i nie miala config-driven hard gate.

Mechanizm bledu:
Bez pola w `SybilResistanceFeatures`, configu, DecisionLoggerze i policy, run mialby tylko czesciowe CPV dane albo progi posrednie,
ale nie audytowalny warunek `cpv_other_pool_activity > 8.5`.

Miejsce:
CPV computation, feature materialization, Gatekeeper V2 config, Gatekeeper strict threshold policy, decision logging.

Skutek:
R37 nie mogl byc uruchomiony zgodnie z dyspozycja uzytkownika bez zmiany kodu.

Dowod:
Przed patchem `rg min_cpv_other_pool_activity` nie znajdowal aktywnego config/policy path. Po patchem testy Gatekeepera pokrywaja low
oraz missing CPV other-pool activity.

Odrzucone hipotezy:
- "Mozna uzyc istniejacego signer_cross_pool_velocity": odrzucone, bo to inna semantyka progu.
- "Mozna ustawic prog tylko w configu": odrzucone, bo bez field/policy path config nie bylby egzekwowany.
- "Missing mozna traktowac jako zero bez reason": odrzucone, bo aktywny hard gate ma fail-closed i audytowalny reason.

## 5. Strategia naprawy

Przyjeta strategia:
Dodac minimalny, jawny field `cpv_other_pool_activity` w istniejacej rodzinie CPV, materializowac go do SSOT feature snapshot,
dodac backward-compatible config `min_cpv_other_pool_activity`, egzekwowac go w istniejacej strict metric threshold gate, oraz logowac
wartosc i prog w buy log/selector feature map.

Zakres ingerencji:
- Jedno nowe pole CPV.
- Jedno nowe pole configu.
- Jedna nowa strict-threshold check branch.
- Logging/feature map/test fixtures.
- Dwa nowe configi R37.

Czego nie zmieniano:
- Execution/send path.
- Live execution.
- Funding lane/FSC.
- R2 label semantics.
- Shadow lifecycle semantics.
- Istniejacy threshold order poza dodaniem CPV check w strict metric gate.
- Scoring/model logic.
- Stare R36 artefakty.

Ryzyka:
- Threshold `8.5` moze byc bardzo selektywny; R37 nie wygenerowal klasycznego BUY lifecycle w pierwszym proof window.
- Standardowy lifecycle launcher proof patrzy na klasyczny BUY path, nie na p37 all-decision probe, wiec moze failowac przy profilach celowo odrzucajacych prawie wszystko.
- CPV coverage zalezy od materializacji CPV history; missing przy aktywnym progu failuje celowo.

Odrzucone alternatywy:
- Hardcodowanie progu `8.5` w policy: odrzucone, threshold musi byc config-driven.
- Reuzycie `signer_cross_pool_velocity`: odrzucone, inna metryka.
- Zmiana DecisionLogger schema destrukcyjnie: odrzucone, dodano pole opcjonalne/backward-compatible.
- Zatrzymanie R37 po BUY lifecycle proof fail: odrzucone na tym etapie, bo all-decision probe emituje dane i runtime pozostaje zdrowy.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs`
- Co zmieniono: dodano `cpv_other_pool_activity: Option<f64>` do `CpvComputation` i wyliczenie sredniej liczby odrebnych innych pooli na current signer sample.
- Dlaczego: potrzebny byl decision-time-safe sygnal cross-pool aktywnosci innej niz obecny pool.
- Efekt: CPV computation zwraca jawny field, `None` dla degraded compute i wartosc liczbowa dla zdrowej probki.

Zmiana 2:
- Plik/modul: `ghost-core/src/tx_intelligence/types.rs`, `ghost-launcher/src/session/observation.rs`
- Co zmieniono: dodano opcjonalne `cpv_other_pool_activity` do `SybilResistanceFeatures` i materializacji.
- Dlaczego: Gatekeeper policy ma konsumowac SSOT feature snapshot, nie recompute z bocznych zrodel.
- Efekt: field jest dostepny w materialized decision snapshot.

Zmiana 3:
- Plik/modul: `ghost-brain/src/config/ghost_brain_config.rs`
- Co zmieniono: dodano `#[serde(default)] pub min_cpv_other_pool_activity: f64` z defaultem `0.0`.
- Dlaczego: threshold ma byc config-driven i backward-compatible.
- Efekt: stare configi pozostaja ladowalne; prog jest nieaktywny, dopoki wartosc nie jest dodatnia.

Zmiana 4:
- Plik/modul: `ghost-launcher/src/components/gatekeeper_policy.rs`
- Co zmieniono: strict metric threshold gate sprawdza `min_cpv_other_pool_activity`; missing/nonfinite lub wartosc `<= threshold` daje hard fail.
- Dlaczego: dyspozycja wymagala, aby niespelnienie choc jednego progu dawalo REJECT.
- Efekt: `cpv_other_pool_activity > 8.5` jest egzekwowane fail-closed w R37.

Zmiana 5:
- Plik/modul: `ghost-launcher/src/components/gatekeeper.rs`, `ghost-brain/src/oracle/decision_logger.rs`
- Co zmieniono: dodano logging wartosci i progu CPV, fingerprint summary oraz `gk_cpv_other_pool_activity` w selector feature map.
- Dlaczego: decyzja musi byc pozniej audytowalna z JSONL.
- Efekt: R37 decision logs zawieraja pole/prog i mozna analizowac jego coverage/fail behavior.

Zmiana 6:
- Plik/modul: testy Gatekeeper/CPV
- Co zmieniono: dodano testy CPV other-pool activity oraz low/missing hard-gate cases; zaktualizowano fixture JSON.
- Dlaczego: zabezpieczenie przed regresja w egzekucji progu i logging contract.
- Efekt: targetowane testy przechodza.

Zmiana 7:
- Plik/modul: R37 configi rollout
- Co zmieniono: utworzono R37 profile na bazie R36, zmieniajac `max_jito_tip_intensity=0.33617` i dodajac `min_cpv_other_pool_activity=8.5`.
- Dlaczego: R37 ma testowac nowy prog przy pozostalych ustawieniach R36.
- Efekt: R37 runtime dziala w tmux `r37-threshold-probe-target50-stop50`.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Unit - strict gate | `cargo test -p ghost-launcher strict_metric_threshold_gate -- --nocapture` | 4 tests passed | PASS | new low/missing CPV gate tests passed |
| Unit - CPV field | `cargo test -p ghost-launcher cpv_other_pool_activity -- --nocapture` | 3 tests passed | PASS | CPV activity computation + gate tests passed |
| Format | `rustfmt --edition 2021 ...` | touched Rust files formatted | PASS | command exit 0 |
| Build | `cargo build --release -p ghost-launcher` | release build completed, warnings only | PASS | build finished in release profile |
| Diff hygiene | `git diff --check` | no whitespace errors before ADR | PASS | command exit 0 |
| R36 audit | generated/read `R36_RUNTIME_AUDIT_SUMMARY.md` | R36 coverage summarized | PASS | report exists under R36 report folder |
| R36 stop | `tmux kill-session -t r36-threshold-probe-target50-stop50` | R36 tmux session removed | PASS | later `tmux ls` has no R36 session |
| R37 config grep | `grep -n ... r37...toml` | `max_jito_tip_intensity=0.33617`, `min_cpv_other_pool_activity=8.5` | PASS | R37 Ghost Brain config |
| R37 event canary | launcher event canary | event canary PASS | PASS | `event_canary/RUN_LIFECYCLE_CANARY_PROOF.md` |
| R37 BUY lifecycle canary | launcher lifecycle canary | no classic BUY lifecycle in proof window | FAIL_EXPECTED_FOR_STRICT_PROFILE | `lifecycle_canary/RUN_LIFECYCLE_CANARY_PROOF.md` |
| R37 all-decision probe | first R37 artifact snapshot | probe selection/transport/entries/lifecycle non-empty | PASS | `probe_selection`, `probe_transport`, `probe_shadow_entries`, `probe_shadow_lifecycle` |
| R37 runtime state | `tmux ls`, `pgrep -af ghost-launcher` | R37 runtime running | PASS | `r37-threshold-probe-target50-stop50`, pid observed |

Wniosek walidacyjny:
Kod i config dla `cpv_other_pool_activity > 8.5` sa wdrozone i przetestowane targetowo. R37 zostal uruchomiony i zbiera all-decision
probe artifacts. Formalny BUY lifecycle guard nie przeszedl, poniewaz strict R37 nie wygenerowal klasycznego BUY w proof window; nie
uniewaznia to all-decision probe, ale oznacza, ze R37 nie ma klasycznego BUY lifecycle proof na starcie.

Ograniczenia walidacji:
- R37 nie jest jeszcze finalnym datasetem ani labelowanym runem.
- R37 BUY-path lifecycle proof pozostaje FAIL na starcie.
- Nie oceniano biznesowego edge ani precision.
- Nie uruchamiano FSC; `funding_lane_mode=disabled`.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: config default / backward compatibility
- Co zabezpiecza: stare configi bez `min_cpv_other_pool_activity`
- Kiedy sie aktywuje: deserializacja Gatekeeper V2 configu
- Jak przetestowano: targetowane testy/build; default `0.0` jest inactive
- Co pozostaje poza zakresem: jakosc statystyczna progu

Guardrail 2:
- Typ: fail-closed strict metric threshold
- Co zabezpiecza: brak lub zbyt niska wartosc CPV other-pool activity przy aktywnym progu
- Kiedy sie aktywuje: `min_cpv_other_pool_activity > 0.0`
- Jak przetestowano: low CPV i missing CPV tests
- Co pozostaje poza zakresem: coverage CPV w dlugim runtime

Guardrail 3:
- Typ: SSOT materialization
- Co zabezpiecza: Gatekeeper policy nie recompute CPV z bocznych zrodel
- Kiedy sie aktywuje: `PoolObservationSession::materialize_features()`
- Jak przetestowano: pole przechodzi przez `SybilResistanceFeatures`, build/test PASS
- Co pozostaje poza zakresem: pelny replay parity dla historycznych runow

Guardrail 4:
- Typ: DecisionLogger/audit evidence
- Co zabezpiecza: pozniejszy audyt decyzji R37
- Kiedy sie aktywuje: gatekeeper buy/decision JSONL emission
- Jak przetestowano: R37 decision rows zawieraja CPV field/prog
- Co pozostaje poza zakresem: finalny selector dataset po zamknieciu R37

## Otwarte ryzyka / follow-up

- R37 standardowy BUY lifecycle proof ma status fail/no-BUY; monitorowac, czy klasyczny BUY pojawi sie pozniej, ale nie restartowac bez decyzji.
- Dla R37 podstawowa sciezka do obserwacji to all-decision probe coverage, nie klasyczny BUY lifecycle proof.
- Jezeli `min_cpv_other_pool_activity=8.5` utrzyma bardzo niska selekcje, potrzebna bedzie decyzja, czy to jest oczekiwane jako stress profile.
- Po zamknieciu R37 zbudowac finalny all-decision universe/labels i policzyc oddzielnie TARGET/STOP/TIMEOUT dla probe-selected i probe-skipped.
- Nie commitowac runtime reports/logs/datasets; commitowalne sa kod, configi i ADR, jesli uzytkownik pozniej to zatwierdzi.
