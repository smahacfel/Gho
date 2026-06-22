# ADR-8D: R35 strict threshold hard gate repair

Status: IMPLEMENTED / R35_STOPPED_INVALIDATED
Typ: ADR-8D / runtime policy guard / config compatibility / regression repair
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: /root/Gho / backup-vps
Commit/PR: local working tree; not committed at report time
Zakres: Gatekeeper V2/V2.5 strict metric threshold semantics for R35 threshold probe
Dotkniete moduly/pliki:
- ghost-brain/src/config/ghost_brain_config.rs
- ghost-brain/src/oracle/reason_code.rs
- ghost-launcher/src/components/gatekeeper_policy.rs
- ghost-launcher/src/components/gatekeeper.rs
- ghost-launcher/src/oracle_runtime.rs
- configs/rollout/ghost_brain_selector_dataset_sampler_r35_threshold_probe_maxwait3789_fsc_off.toml
- configs/rollout/shadow-burnin-v3-r35-threshold-probe-target50-stop50-fsc-off-r1.toml
Powiazane runy/logi/raporty:
- logs/rollout/shadow-burnin-v3-r35-threshold-probe-target50-stop50-fsc-off-r1/launcher.stdout.log
- logs/rollout/shadow-burnin-v3-r35-threshold-probe-target50-stop50-fsc-off-r1/decisions/
- logs/shadow_run/shadow-burnin-v3-r35-threshold-probe-target50-stop50-fsc-off-r1/probe_selection.jsonl
- logs/shadow_run/shadow-burnin-v3-r35-threshold-probe-target50-stop50-fsc-off-r1/probe_transport.jsonl
- logs/shadow_run/shadow-burnin-v3-r35-threshold-probe-target50-stop50-fsc-off-r1/probe_shadow_entries.jsonl
- logs/shadow_run/shadow-burnin-v3-r35-threshold-probe-target50-stop50-fsc-off-r1/probe_shadow_lifecycle.jsonl
Poziom ryzyka: HIGH for R35 validity; MEDIUM after code repair

## 1. Przygotowanie i działania wstępne

Plan początkowy:
Uruchomic R35 jako progowy Gatekeeper probe, bazujacy na R34, ale z progiem obserwacji 3789 ms oraz kontraktem:
min_tx_count=5, min_unique_signers=2, min_buy_count=2, signer_cross_pool_velocity < 0.9737,
jito_tip_intensity < 0.3485, flipper_presence_ratio >= 0.19, burst_ratio >= 0.27.
Uzytkownik doprecyzowal, ze niespelnienie choc jednego z tych warunkow ma skutkowac decyzja REJECT.

Rzeczywisty przebieg:
R35 zostal uruchomiony, ale w trakcie status checku wykryto, ze runtime nie egzekwowal kontraktu jako jednego twardego AND gate.
Czesc wartosci z configu byla nieaktywna, bo pola nie istnialy w `GatekeeperV2Config`, a czesc sciezek deadline mogla nadal
emitowac TIMEOUT dla shortfallu Phase1 zamiast REJECT. R35 zostal zatrzymany i uznany za niewazny jako walidacja progow.
Nastepnie dodano jawny config-driven strict hard gate, typed reason code oraz guardy deadline/feature-driven terminal path.

Odchylenia od planu:
- Zamiast kontynuowac R35, run zostal zatrzymany i uniewazniony.
- Dodano brakujace pola configu, poniewaz samo ustawienie TOML bylo czesciowo ignorowane.
- Dodano nowy typed reason code `HARD_FAIL_STRICT_METRIC_THRESHOLD`.
- Nie restartowano R35 po poprawce; restart wymaga osobnej dyspozycji.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powód użycia:
Zmiana dotyka aktywnej sciezki Gatekeeper V2/V2.5, typed verdict/reason-code discipline, DecisionLogger evidence i shadow/live boundary.
Zakres użycia:
Utrzymano SSOT `GatekeeperAssessment` / `MaterializedFeatureSet`, fail-closed missing semantics i brak zmian execution/send path.
Wynik:
Strict threshold gate jest jawnie wlaczany flaga configu i nie zmienia starych profili.
Ograniczenia:
Skill nie zastepuje runtime validation; R35 pozostaje zatrzymany i wymaga ponownego smoke po akceptacji.

Nazwa: rust-master
Powód użycia:
Zmiana obejmuje Rust config struct, enum reason code, policy helper, deadline paths i testy.
Zakres użycia:
Minimalny patch bez szerokiego refactoru, z kompatybilnymi defaultami i targeted tests dla negatywnych przypadkow.
Wynik:
Kod kompiluje sie w `cargo check -p ghost-launcher`, a testy strict gate przechodza.
Ograniczenia:
Nie uruchamiano pelnego workspace test suite.

## 3. Opis problemu — 3W2H

What:
R35 mial stosowac zestaw progow jako twardy AND gate: kazdy token niespelniajacy dowolnego progu powinien dostac REJECT.
W praktyce czesc progow nie byla egzekwowana, a shortfall Phase1 mogl konczyc sie TIMEOUT.

Where:
Aktywna sciezka Gatekeeper V2/V2.5:
- `GatekeeperV2Config`
- `evaluate_hard_filters_from_assessment`
- `GatekeeperBuffer::compute_decision`
- `GatekeeperBuffer::check_standard_deadline`
- `GatekeeperBuffer::check_long_deadline`
- feature-driven terminal path w `oracle_runtime.rs`

Why it matters:
Run progowy nie odpowiada definicji testu, jezeli progi sa traktowane czesciowo jako telemetry/soft/degraded albo deadline timeout.
Takie dane nie moglyby byc uzyte do oceny hipotezy uzytkownika i prowadzilyby do falszywej interpretacji BUY/REJECT.

How observed:
Status check R35 pokazal, ze profile z niespelnionymi warunkami nie sa konsekwentnie odrzucane jako strict threshold REJECT.
Audit configu i kodu pokazal, ze `strict_metric_threshold_gate_enabled`, `min_burst_ratio` i `min_flipper_presence_ratio`
nie byly polami `GatekeeperV2Config`, wiec TOML nie mial pelnej mocy decyzyjnej.

How many / scale:
Dotyczy kazdej decyzji w profilach, ktore maja wymagac strict threshold semantics. R35 jako dotychczasowy run jest niewazny
dla walidacji tych progow. Domyslnie stare profile pozostaja bez zmian przez `strict_metric_threshold_gate_enabled=false`.

Evidence:
- R35 process/sesja zostaly zatrzymane; `tmux has-session -t r35-threshold-probe-target50-stop50` nie znajduje sesji.
- W logice configu przed poprawka brakowalo czesci pol TOML.
- Dodane testy wymuszaja missing metric => hard reject oraz Phase1 shortfall => REJECT, nie TIMEOUT.

## 4. Przyczyna źródłowa

Root cause:
Brak jednego jawnego kontraktu strict-threshold AND gate w aktywnej sciezce Gatekeepera.

Mechanizm błędu:
Konfiguracja R35 zawierala intencje biznesowa, ale czesc pol nie istniala w config struct i byla ignorowana przez deserializacje.
Dodatkowo dotychczasowa semantyka Phase1/deadline mogla klasyfikowac brak minimalnego evidence jako TIMEOUT zamiast hard REJECT.

Miejsce:
- `ghost-brain/src/config/ghost_brain_config.rs`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/oracle_runtime.rs`

Skutek:
R35 nie egzekwowal "niespelnienie dowolnego progu => REJECT" w sposob wiarygodny. Dane z pierwotnego R35 nie powinny byc traktowane
jako poprawny wynik threshold probe.

Dowód:
Po patchu helper `strict_metric_threshold_failure_from_assessment` fail-closed odrzuca missing/out-of-range metrics, a test
`test_force_check_deadline_long_mode_strict_threshold_rejects_phase1_shortfall` wymusza REJECT dla count shortfallu.

Odrzucone hipotezy:
- "To tylko zly config": odrzucone, bo config zawieral pola bez odpowiednikow w struct.
- "Timeout jest akceptowalnym skutkiem shortfallu": odrzucone, bo uzytkownik zdefiniowal hard REJECT.
- "Missing mozna traktowac jako zero/safe": odrzucone, bo narusza fail-closed i zaciemnia metryki.
- "Trzeba zmienic execution/send path": odrzucone, problem byl w policy/deadline semantics.

## 5. Strategia naprawy

Przyjęta strategia:
Dodac jawny, opt-in strict gate w `GatekeeperV2Config`, ktory w profilach z flaga `strict_metric_threshold_gate_enabled=true`
sprawdza wszystkie wymagane progi jako AND gate i zwraca typed hard REJECT dla missing/out-of-range values.

Zakres ingerencji:
Minimalna zmiana w configu, reason-code enum, hard-filter helperze, deadline paths i feature-driven terminal path.

Czego nie zmieniano:
- Gatekeeper policy poza strict gate.
- Execution.
- Send path.
- Live/shadow boundary.
- Selector/model/scoring.
- FSC.
- Wartosc progow R35 poza tym, co uzytkownik zdefiniowal.

Ryzyka:
- Strict fail-closed moze mocno obnizyc BUY count; to jest oczekiwany efekt profilu progowego, nie regresja.
- Deserializacja nadal moze tolerowac inne unknown TOML fields; ten patch naprawia konkretna semantyke R35, nie wprowadza globalnego `deny_unknown_fields`.
- Pelny workspace test suite nie zostal uruchomiony.

Odrzucone alternatywy:
- Hardcodowac progi w policy: odrzucone, bo progi decyzyjne maja byc config-driven.
- Globalnie zmienic Phase1 timeout na REJECT: odrzucone, bo naruszyloby stare profile.
- Uruchomic kolejny run bez naprawy kodu: odrzucone, bo kontynuowaloby wadliwy kontrakt.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/moduł: `ghost-brain/src/config/ghost_brain_config.rs`
- Co zmieniono: dodano `min_burst_ratio`, `min_flipper_presence_ratio`, `strict_metric_threshold_gate_enabled`.
- Dlaczego: R35 wymaga minimalnych progow i jawnego wlacznika strict mode.
- Efekt: stare configi zachowuja kompatybilnosc, a R35 moze wlaczyc strict hard gate.

Zmiana 2:
- Plik/moduł: `ghost-brain/src/oracle/reason_code.rs`
- Co zmieniono: dodano `HardFailStrictMetricThreshold` i mapping z `StrictMetricThreshold`.
- Dlaczego: terminalne REJECT-y musza byc audytowalne przez typed reason code.
- Efekt: logi/DecisionLogger moga rozroznic strict threshold reject od innych hard fail.

Zmiana 3:
- Plik/moduł: `ghost-launcher/src/components/gatekeeper_policy.rs`
- Co zmieniono: dodano `HardFailReason::StrictMetricThreshold`, helper `strict_metric_threshold_failure_from_assessment`,
  min burst/flipper checks oraz testy missing/passing metrics.
- Dlaczego: policy layer musi egzekwowac wszystkie progi jako AND gate i fail-closed dla missing.
- Efekt: brak lub niespelnienie metryki w strict mode daje `HARD_FAIL_STRICT_METRIC_THRESHOLD`.

Zmiana 4:
- Plik/moduł: `ghost-launcher/src/components/gatekeeper.rs`
- Co zmieniono: `compute_decision`, `check_standard_deadline` i `check_long_deadline` respektuja strict helper;
  Phase1/count shortfall w strict mode zwraca REJECT zamiast TIMEOUT.
- Dlaczego: count gates sa czescia wymagan uzytkownika.
- Efekt: `min_tx_count`, `min_unique_signers`, `min_buy_count` sa egzekwowane jako hard reject w R35.

Zmiana 5:
- Plik/moduł: `ghost-launcher/src/oracle_runtime.rs`
- Co zmieniono: feature-driven terminal verdict path sprawdza strict helper przed zbudowaniem timeout decision.
- Dlaczego: runtime terminal path nie moze omijac policy kontraktu.
- Efekt: jedna semantyka strict REJECT obowiazuje takze w feature-driven deadline path.

## 7. Walidacja działań naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowód |
|---|---|---|---|---|
| Stop R35 | `tmux has-session -t r35-threshold-probe-target50-stop50` | `can't find session` | PASS | R35 nie dziala jako sesja tmux |
| Process check | `pgrep -af "target/release/ghost-launcher"` | brak realnego `ghost-launcher`; match dotyczy command line procesu tmux sesji Codex | PASS | brak aktywnego run process |
| Format | `rustfmt --edition 2021 <touched rust files>` | zakonczone bez bledu | PASS | format tylko touched files |
| Whitespace | `git diff --check` | brak bledow | PASS | command exit 0 |
| Unit | `cargo test -p ghost-launcher strict_metric_threshold_gate -- --nocapture` | 2 passed | PASS | missing metric reject + passing metrics allow |
| Guard negative case | `cargo test -p ghost-launcher test_force_check_deadline_long_mode_strict_threshold_rejects_phase1_shortfall -- --nocapture` | 1 passed | PASS | strict Phase1 shortfall => REJECT |
| Compile | `cargo check -p ghost-launcher` | finished dev profile; warnings only | PASS | command exit 0 |

Wniosek walidacyjny:
Poprawka kodu spelnia kontrakt: w profilach z `strict_metric_threshold_gate_enabled=true` niespelnienie dowolnego wymaganego progu
albo brak wymaganej metryki konczy sie typed hard REJECT. R35 sprzed poprawki pozostaje niewazny i nie zostal ponownie uruchomiony.

Ograniczenia walidacji:
- Nie uruchomiono pelnego workspace test suite.
- Nie wykonano nowego runtime smoke po patchu, zgodnie z tym, ze najpierw run zostal zatrzymany i naprawa miala byc domknieta.
- Istniejace warningi w `cargo check` sa poza zakresem tej zmiany.

## 8. Wdrożone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: config opt-in guard
- Co zabezpiecza: stare profile przed niejawna zmiana semantyki Gatekeepera
- Kiedy się aktywuje: strict gate dziala tylko przy `strict_metric_threshold_gate_enabled=true`
- Jak przetestowano: helper zwraca `None`, gdy flaga jest false; targeted tests aktywuja ja jawnie
- Co pozostaje poza zakresem: globalne wykrywanie wszystkich unknown TOML fields

Guardrail 2:
- Typ: typed reason-code guard
- Co zabezpiecza: audytowalnosc terminalnego strict REJECT
- Kiedy się aktywuje: `HardFailReason::StrictMetricThreshold`
- Jak przetestowano: testy sprawdzaja `GatekeeperReasonCode::HardFailStrictMetricThreshold`
- Co pozostaje poza zakresem: downstream dashboard rendering

Guardrail 3:
- Typ: fail-closed missing-data guard
- Co zabezpiecza: missing metric nie jest zero ani safe
- Kiedy się aktywuje: missing burst/flipper/jito/cross-pool velocity w strict mode
- Jak przetestowano: `strict_metric_threshold_gate_rejects_missing_required_metric`
- Co pozostaje poza zakresem: przyczyny missing w ingest/materialization

Guardrail 4:
- Typ: deadline-path guard
- Co zabezpiecza: Phase1/count shortfall nie przechodzi jako TIMEOUT w strict mode
- Kiedy się aktywuje: standard/long deadline oraz feature-driven deadline path
- Jak przetestowano: `test_force_check_deadline_long_mode_strict_threshold_rejects_phase1_shortfall`
- Co pozostaje poza zakresem: dlugoterminowa statystyka po ponownym runtime smoke

## Otwarte ryzyka / follow-up

- R35 nalezy odpalic od nowa dopiero po jawnej decyzji uzytkownika; poprzedni R35 jest invalid.
- Po restarcie R35 trzeba policzyc verdict breakdown i potwierdzic, ze strict violations trafiaja do `HARD_FAIL_STRICT_METRIC_THRESHOLD`.
- Warto osobno rozwazyc guard na unknown config fields dla rolloutow progowych, bo to wlasnie ukrylo czesc intencji TOML.
- Nie mieszac tej naprawy z unrelated WIP widocznym w worktree.
