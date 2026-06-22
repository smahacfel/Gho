# ADR-8D: R38 P37 shadow-probe decision-time entry anchor

Status: IMPLEMENTED / TARGETED_TESTS_PASS
Typ: ADR-8D / shadow-probe lifecycle timing repair / decision-time entry anchor
Data: 2026-06-19
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
Commit/PR: not committed at report time
Zakres: P37 counterfactual shadow-probe, active shadow handoff, post-buy lifecycle entry timestamp semantics
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/components/post_buy_runtime.rs`
- `ghost-brain/src/guardian/post_buy/engine.rs`
- `ghost-brain/src/pipeline/execution.rs`
- `ghost-brain/src/pipeline/jito_processor.rs`
- `docs/ADR/ADR_8D_R38_P37_SHADOW_PROBE_DECISION_TIME_ENTRY_ANCHOR_20260619.md`

Powiazane runy/logi/raporty:
- User-provided R38 record excerpt for pool `BQYYX6WkGVWK3A7ABwVU8fnBd83WHDZ92eabUzGS3BAv`
- R38 profile: `shadow-burnin-v3-r38-threshold-probe-target50-stop50-fsc-off-r1`
- R38 brain config path from record:
  `/root/Gho/configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`
- Probe lifecycle artifact from record:
  `probe_shadow_lifecycle.jsonl`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Dokument zachowuje lokalny format ADR-8D uzywany w repo.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zweryfikowac, dlaczego P37/R38 counterfactual shadow-probe lifecycle pokazal wejscie w okolicy slotu `427356037`, mimo ze terminalny Gatekeeper record byl hard-fail reject:
- `core_pass=false`
- `hard_fail_reason="HARD_FAIL: strict_metric_threshold unique_signers=3/4"`
- `observation_start_ts_ms=1781812685960`
- `observation_end_ts_ms=1781812717060`
- `entry_slot=427356037`
- `_lifecycle_source_file="probe_shadow_lifecycle.jsonl"`

Rzeczywisty przebieg:
- Sprawdzono, ze wpis pochodzi z `probe_shadow_lifecycle.jsonl`, czyli P37 counterfactual probe plane.
- Skorygowano interpretacje: P37 probe jest celowym, wymaganym lifecycle trackingiem tokenow odrzuconych przez Gatekeeper i nie moze byc usuwany ani stale-skipowany tylko dlatego, ze decyzja byla reject.
- Sprawdzono mapowanie `P37ShadowProbeCandidate` z Gatekeeper log row.
- Sprawdzono P37 transport/entry row materialization.
- Sprawdzono `GhostEvent::PostBuySubmitted` handoff i `PostBuyRuntime` do `MonitoringEngine`.
- Sprawdzono, gdzie PostBuyGuardian kotwiczy `entry_unix_ms`, `last_peak_unix_ms`, `PositionOpenedPayload.entry_time_ms` i close duration.
- Naprawa zostala przeniesiona z blednego "skip stale probe" na poprawne "probe zostaje, ale entry/lifecycle dostaje decision-time anchor".

Odchylenia od planu:
Dokladny user-provided pool/mint nie zostal odnaleziony w lokalnych artefaktach checkoutu. Diagnoza opiera sie na wklejonym rekordzie oraz aktualnym kodzie/configu.

## 2. Wykorzystane skills/specjalisci

Nazwa:
`ghost-execution`

Powod uzycia:
Zmiana dotyczy shadow/live separation, P37 probe artifacts, DecisionLogger auditability i lifecycle semantics.

Wynik:
Naprawa zachowuje P37 counterfactual probe jako osobny shadow/probe plane i nie miesza go z active BUY.

Nazwa:
`solana-pumpfun-architect`

Powod uzycia:
Problem objawial sie jako roznica miedzy oczekiwanym decision-time entry slot/time a pozniejszym slotem symulacji.

Wynik:
Nie zmieniano TX buildera ani sendera. Symulacja moze wystapic pozniej, ale lifecycle entry timestamp jest teraz kotwiczony do decyzji/kontrafaktycznego wejscia.

Nazwa:
`rust-master`

Powod uzycia:
Zmiana przechodzi przez event enum, async handoff i shared guardian state.

Wynik:
Dodano jawne opcjonalne pole w handoffie, bez ukrytego globalnego stanu, bez nowych retry loopow i bez blokowania hot path.

## 3. Opis problemu - 3W2H

What:
P37 probe lifecycle row pokazywal entry/lifecycle czas z momentu wykonania symulacji albo rejestracji pozycji w PostBuyGuardian, a nie z momentu, ktory powinien byc semantycznym wejscem counterfactual shadow/probe.

Where:
- `ghost-launcher/src/oracle_runtime.rs`, P37 shadow entry record i handoff do post-buy.
- `ghost-launcher/src/events.rs`, `GhostEvent::PostBuySubmitted`.
- `ghost-launcher/src/components/post_buy_runtime.rs`, shadow/probe handoff.
- `ghost-brain/src/guardian/post_buy/engine.rs`, `MonitoringEngine::register_position_with_context()` i close duration.

Why it matters:
`probe_shadow_lifecycle.jsonl` ma sluzyc do porownania losu tokenow zaakceptowanych i odrzuconych przez Gatekeeper. Jezeli entry timestamp jest czasem symulacji/rejestracji zamiast decision-time anchor, PnL, duration, timeout, lifecycle proof i porownanie z chainem sa przesuniete i mylace.

How observed:
User-provided R38 row pokazywal hard fail/reject, ale probe lifecycle mial entry slot okolo `427356037`, czyli pozniej niz oczekiwany punkt wejscia na chainie. Jednoczesnie byl to poprawny P37 probe artifact, nie bledny active BUY.

How many / scale:
Mechanizm byl ogolny dla active shadow i P37 probe handoffow do PostBuyGuardian. Skala historycznych row nie zostala policzona lokalnie.

## 4. Przyczyna zrodlowa

Root cause 1:
`P37ShadowProbeCandidate::from_gatekeeper_log()` preferowal `ab_t_end_event_ts_ms` przed `observation_end_ts_ms` jako `decision_ts_ms`.

Mechanizm:
Dla dlugiego R38 okna `observation_end_ts_ms` jest terminalnym koncem obserwacji, a `ab_t_end_event_ts_ms` moze odpowiadac krotszemu aliasowi/early window. W user record widac okolice `1781812687953/1781812687960`, czyli okolo 2 sekundy po first seen, zamiast terminalnego `1781812717060`.

Root cause 2:
P37 shadow entry row uzywal czasu wykonania symulacji jako `timestamp_ms`.

Mechanizm:
To mieszalo "kiedy symulacja sie wykonala" z "kiedy kontrfaktyczna pozycja miala byc otwarta". Taki row wygladal jak realniejsze pozne wejscie, chociaz P37 probe jest analiza kontrfaktyczna.

Root cause 3:
`PostBuyRuntime` nie przekazywal semantycznego czasu otwarcia pozycji do `MonitoringEngine`, a `MonitoringEngine::register_position_with_context()` inicjalizowal pozycje na `current_time_ms()`.

Mechanizm:
Nawet jezeli upstream mial poprawny `decision_ts_ms`, guardian-side lifecycle state (`entry_unix_ms`, `last_peak_unix_ms`, `PositionOpenedPayload.entry_time_ms`, `RegisteredPosition.opened_at_ms`) bylo kotwiczone do czasu rejestracji w monitoringu.

Root cause 4:
`unregister_position()` liczyl duration z `pos.entry_time.elapsed()`, czyli z lokalnego `Instant` utworzonego przy rejestracji, a nie z materialnego `entry_unix_ms`.

Mechanizm:
Dla pozycji syntetycznych/counterfactual duration powinien wynikac z decision-time entry anchor, nie z lokalnego wall-clock czasu rejestracji monitoringu.

Odrzucona hipoteza:
`fraction_bps=10000` nie jest przyczyna laga. To 10000 basis points, czyli 100% frakcji pozycji w lifecycle accounting. Nie steruje czasem wejscia, slotem symulacji ani schedulingiem.

## 5. Strategia naprawy

Przyjeta strategia:
- P37 probe zostaje. Nie jest skipowany za sam fakt rejectu ani za age.
- P37 candidate preferuje terminalne `observation_end_ts_ms` jako `decision_ts_ms`.
- P37/active shadow entry row zapisuje `timestamp_ms=decision_ts_ms` i `timing_source="decision_ts_ms"`.
- `GhostEvent::PostBuySubmitted` dostaje opcjonalne `entry_opened_at_ms`.
- Active shadow i P37 probe handoff przekazuja `Some(decision_ts_ms)` jako `entry_opened_at_ms`.
- `PostBuyRuntime` przenosi `entry_opened_at_ms` do `PositionEventContext.opened_at_ms`.
- `MonitoringEngine::register_position_with_context()` uzywa `opened_at_ms` jako lifecycle entry anchor, jezeli jest ustawiony i dodatni.
- `unregister_position()` liczy duration z `current_time_ms().saturating_sub(pos.entry_unix_ms)`.
- P37 age zostaje jako diagnostyka (`probe_age_status="fresh"` albo `"delayed"`), nie jako gate eligibility.

Czego nie zmieniano:
- Gatekeeper policy.
- Strict metric thresholds.
- `core_pass` semantics.
- TX builder.
- live sender / Helius Sender.
- Shadow probe existence.
- P37 sampling/selection intent.
- Config defaults.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `ghost-launcher/src/oracle_runtime.rs`
- Funkcja: `P37ShadowProbeCandidate::from_gatekeeper_log`
- Co zmieniono: `decision_ts_ms` preferuje teraz `observation_end_ts_ms` przed `ab_t_end_event_ts_ms`.
- Efekt: P37 candidate kotwiczy sie do terminalnego konca obserwacji, a nie do early/AB aliasu.

Zmiana 2:
- Plik/modul: `ghost-launcher/src/oracle_runtime.rs`
- Funkcje: `p37_shadow_probe_max_entry_age_ms`, `p37_shadow_probe_age_status`
- Co zmieniono: age jest klasyfikowany jako `fresh`/`delayed`.
- Efekt: opozniony probe jest audytowalny, ale nie jest blokowany jako lifecycle evidence.

Zmiana 3:
- Plik/modul: `ghost-launcher/src/oracle_runtime.rs`
- Funkcje: `shadow_entry_record_from_event`, `shadow_entry_record_from_request`
- Co zmieniono: `timestamp_ms` entry row jest `decision_ts_ms`; `timing_source` jest `decision_ts_ms`.
- Efekt: entry row pokazuje semantyczny entry anchor, a nie czas zakonczenia symulacji.

Zmiana 4:
- Plik/modul: `ghost-launcher/src/events.rs`
- Typ: `GhostEvent::PostBuySubmitted`
- Co zmieniono: dodano `entry_opened_at_ms: Option<u64>` i builder `with_entry_opened_at_ms`.
- Efekt: shadow/probe handoff moze przeniesc decision-time entry anchor do post-buy runtime bez zmiany istniejacej semantyki live call-siteow.

Zmiana 5:
- Plik/modul: `ghost-launcher/src/components/post_buy_runtime.rs`
- Funkcje: `handle_post_buy_event`, `handle_shadow_post_buy_handoff`
- Co zmieniono: shadow/probe lanes przekazuja `entry_opened_at_ms` do `PositionEventContext.opened_at_ms`.
- Efekt: MonitoringEngine dostaje jawny czas otwarcia pozycji dla active shadow i probe.

Zmiana 6:
- Plik/modul: `ghost-brain/src/guardian/post_buy/engine.rs`
- Funkcje: `PositionEventContext`, `register_position_with_context`, `unregister_position`
- Co zmieniono: dodano `opened_at_ms`; registration uzywa go dla `entry_unix_ms`, `last_peak_unix_ms`, market activity anchor, `PositionOpenedPayload.entry_time_ms` i `RegisteredPosition.opened_at_ms`; close duration liczy z `entry_unix_ms`.
- Efekt: lifecycle state i lifecycle proof trzymaja decision-time entry anchor.

Zmiana 7:
- Pliki/moduly: `ghost-brain/src/pipeline/execution.rs`, `ghost-brain/src/pipeline/jito_processor.rs`
- Co zmieniono: istniejace call-site'y `PositionEventContext` uzupelniono o `opened_at_ms: None`.
- Efekt: stare live/paper/Jito sciezki zachowuja dotychczasowa semantyke i nie dostaja przypadkiem counterfactual entry anchor.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status |
|---|---|---|---|
| Targeted P37 tests | `cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture` | 73 passed, 0 failed | PASS |
| Post-buy runtime tests | `cargo test -p ghost-launcher --lib post_buy_runtime -- --nocapture` | 38 passed, 0 failed | PASS |
| Guardian post-buy tests | `cargo test -p ghost-brain --lib post_buy -- --nocapture` | 42 passed, 0 failed | PASS |

Nowe/zmienione testy:
- `p37_shadow_probe_candidate_prefers_terminal_observation_end_over_ab_alias`
- `p37_shadow_probe_delayed_decision_age_is_diagnostic_only`
- `shadow_handoff_registers_canonical_monitoring_position` rozszerzony o `entry_opened_at_ms` i `PositionOpenedPayload.entry_time_ms`
- `p37_shadow_probe_simulation_error_entry_is_not_lifecycle_eligible` rozszerzony o age fields i `probe_age_status`

Ograniczenia walidacji:
Nie wykonano nowego runtime R38 smoke/replay po rebuildzie w ramach tej zmiany. Pelny proof wymaga nowego runu na binarce zawierajacej patch i sprawdzenia nowych `probe_shadow_lifecycle.jsonl` rows.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: timestamp priority
- Co zabezpiecza: P37 decision timestamp nie cofa sie do early aliasu, gdy istnieje terminalny `observation_end_ts_ms`.
- Jak przetestowano: unit test z `ab_t_end_event_ts_ms=2000` i `observation_end_ts_ms=31100`.

Guardrail 2:
- Typ: probe preservation
- Co zabezpiecza: opozniony P37 probe nie jest blokowany przez age; age pozostaje diagnostyka.
- Jak przetestowano: unit test `p37_shadow_probe_delayed_decision_age_is_diagnostic_only`.

Guardrail 3:
- Typ: entry-time propagation
- Co zabezpiecza: active shadow/probe handoff przenosi `decision_ts_ms` do PostBuyGuardian lifecycle entry.
- Jak przetestowano: post-buy runtime test sprawdza `EventEnvelope.event_time_ms` i `PositionOpenedPayload.entry_time_ms`.

Guardrail 4:
- Typ: lifecycle duration
- Co zabezpiecza: close duration bazuje na `entry_unix_ms`, a nie na lokalnym `Instant` rejestracji.
- Jak przetestowano: guardian post-buy suite 42/42 pass.

## Otwarte ryzyka / follow-up

- Wykonac runtime R38 smoke na nowej binarce i sprawdzic, ze nowe probe lifecycle rows maja entry timestamp z `decision_ts_ms`, a nie z simulation finish/register time.
- Zweryfikowac na artefaktach, ze `probe_age_status="delayed"` wystepuje tylko jako diagnostyka i nie usuwa P37 lifecycle coverage.
- W raportach analitycznych jasno separowac active shadow BUY lifecycle od P37 counterfactual probe lifecycle, ale pozwalac porownywac oba przez wspolny decision-time entry anchor.
