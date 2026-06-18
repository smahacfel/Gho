# ADR-8D: R31 maxwait15000 with FSC disabled runtime profile

Status: Done
Typ: rollout/config
Data: 2026-06-15
Repo/branch: /root/Gho / current dirty worktree
Commit/PR: not committed
Zakres: R31 rollout config, Ghost Brain rollout config, runtime launch
Dotkniete moduly/pliki:
- configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml
- configs/rollout/ghost_brain_selector_dataset_sampler_r31_maxwait15000_fsc_off.toml
Powiazane runy/logi/raporty:
- reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r3/run_lifecycle_guard_20260615T141357Z/
- tmux session: gho-r31
Poziom ryzyka: medium

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
- Utworzyc R31 na bazie R30.
- Zwiekszyc okno Gatekeepera do 15000 ms.
- Wylaczyc FSC.
- Uruchomic run w tmux.

Rzeczywisty przebieg:
- Skopiowano profil R30 do osobnych plikow R31.
- Ustawiono `gatekeeper_v2.max_wait_time_ms = 15000`.
- Ustawiono `gatekeeper_v2.dow.normal_window_ms = 10000` i `extended_window_ms = 15000`, z zachowaniem invariant `extended_window_ms <= max_wait_time_ms`.
- Wylaczono `[fsc_v2]` capture i feature emission.
- Pierwsze proby ujawnily, ze samo `[fsc_v2]` nie wylacza calego FSC storage/runtime path.
- Dodatkowo wylaczono authoritative funding lane oraz NLN Program Streams FSC capture lane.
- Finalny aktywny scope: `shadow-burnin-v3-r31-maxwait15000-fsc-off-r3`.

Odchylenia od planu:
- Uzyto suffixu `-r3`, poniewaz fail-closed `append=false` blokowal ponowne uzycie nieczystych namespace po abortowanych startach.
- Lifecycle launcher zostal odpiety po event canary PASS, poniewaz formalny lifecycle proof nie przeszedl w pierwszych pollach i launcher przy timeout zabilby dzialajacy tmux runtime.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia: zmiana aktywnego shadow rollout configu i runtime path.
Zakres uzycia: ochrona shadow/live boundary, config safety, Gatekeeper/DecisionLogger awareness.
Wynik: zmiany pozostaly config-only i shadow-only.
Ograniczenia: nie diagnozowano w tym ADR przyczyny braku lifecycle proof po wylaczeniu program streams.

## 3. Opis problemu - 3W2H

What:
- Potrzebny byl nowy R31 z dluzszym oknem obserwacji i bez FSC.

Where:
- Launcher config: `configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml`.
- Brain config: `configs/rollout/ghost_brain_selector_dataset_sampler_r31_maxwait15000_fsc_off.toml`.

Why it matters:
- R30 z FSC generowal nieakceptowalny storage pressure.
- R31 ma sprawdzic zachowanie bez FSC oraz z dluzszym oknem 15000 ms.

How observed:
- Preflight release binary potwierdzil `max_wait_ms=15000`.
- Runtime log potwierdzil `funding_lane_mode=disabled`.
- Brak katalogu `logs/nln_capture/shadow-burnin-v3-r31-maxwait15000-fsc-off-r3` po finalnym starcie potwierdzil brak raw FSC capture writer.

How many / scale:
- Event canary PASS dla finalnego scope:
  - `Candidate_delta=7`
  - `NewPoolDetected_delta=30`
  - `PoolTransaction_delta=547`
  - `bad_event_json_delta=0`
  - `diag_account_update_relay_delta=1108`

Evidence:
- `reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r3/run_lifecycle_guard_20260615T141357Z/event_canary/RUN_LIFECYCLE_CANARY_PROOF.md`
- `reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r3/run_lifecycle_guard_20260615T141357Z/runtime.log`

## 4. Przyczyna zrodlowa

Root cause:
- FSC runtime/storage nie bylo kontrolowane jednym przelacznikiem.

Mechanizm bledu:
- `[fsc_v2] capture_enabled=false` i `feature_emit_enabled=false` wylaczaly Brain FSC evidence emission, ale nie wylaczaly:
  - `seer.funding_lane_mode = "full_chain"`
  - `seer.program_streams.enabled = true`
  - raw NLN Program Streams evidence writer

Miejsce:
- `[seer] funding_lane_mode`
- `[seer.program_streams] enabled`
- `[seer.program_streams] artifact_capture_enabled`

Skutek:
- W abortowanych probach R31 runtime nadal logowal FSC authoritative funding coverage i zapisywal `raw_pumpfun_instruction_evidence_v1.jsonl`.

Dowod:
- Abortowany R31-r2 mial log:
  - `Seer: starting NLN Program Streams FSC capture lane`
  - katalog `logs/nln_capture/shadow-burnin-v3-r31-maxwait15000-fsc-off-r2`

Odrzucone hipotezy:
- Samo `fsc_v2.decision_enabled=false` nie wystarcza, bo R30 juz mial FSC policy off, a storage pressure nadal istnial.

## 5. Strategia naprawy

Przyjeta strategia:
- Zachowac profil R30 jako baza.
- Zmienic tylko okna czasowe i przelaczniki zwiazane z FSC/capture path.
- Uzyc osobnego R31 namespace, zeby nie mieszac artefaktow.

Zakres ingerencji:
- Config-only.
- Bez zmian w Rust code.
- Bez live execution.

Czego nie zmieniano:
- Nie zmieniano Gatekeeper policy code.
- Nie zmieniano builderow transakcji.
- Nie zmieniano RPC endpointu.
- Nie zmieniano progow `min_tx_count=3`, `min_unique_signers=2`, `min_buy_count=2`.

Ryzyka:
- Wylaczenie `seer.program_streams.enabled` moze ograniczyc materializacje dodatkowych route/account evidence uzywanych przez probe path.
- Formalny lifecycle proof nie przeszedl w pierwszych pollach po finalnym starcie.

Odrzucone alternatywy:
- Kasowanie abortowanych artefaktow R31 bez potrzeby.
- Pozostawienie `program_streams.enabled=true`, poniewaz generowalo raw FSC capture.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `configs/rollout/ghost_brain_selector_dataset_sampler_r31_maxwait15000_fsc_off.toml`
- Co zmieniono:
  - `fsc_v2.capture_enabled = false`
  - `fsc_v2.feature_emit_enabled = false`
  - `gatekeeper_v2.max_wait_time_ms = 15000`
  - `gatekeeper_v2.dow.normal_window_ms = 10000`
  - `gatekeeper_v2.dow.extended_window_ms = 15000`
- Dlaczego: R31 ma dzialac bez FSC i z dluzszym oknem obserwacji.
- Efekt: release preflight laduje Brain config z `max_wait_ms=15000`.

Zmiana 2:
- Plik/modul: `configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml`
- Co zmieniono:
  - osobny R31 scope `shadow-burnin-v3-r31-maxwait15000-fsc-off-r3`
  - `seer.funding_lane_mode = "disabled"`
  - `seer.program_streams.enabled = false`
  - `seer.program_streams.artifact_capture_enabled = false`
  - wszystkie log/report/data paths przeniesione na R31-r3
- Dlaczego: wylaczenie pelnej FSC/funding capture path i unikniecie nieczystych namespace.
- Efekt: runtime startuje bez raw FSC capture directory dla finalnego R31-r3.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Release preflight | `target/release/ghost-launcher --config configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml --preflight` | all runtime checks passed, `max_wait_ms=15000` | PASS | terminal 2026-06-15T14:13:40Z |
| Event canary | lifecycle launcher, scope `shadow-burnin-v3-r31-maxwait15000-fsc-off-r3` | event canary PASS | PASS | `event_canary/RUN_LIFECYCLE_CANARY_PROOF.md` |
| FSC funding lane off | runtime log grep | `funding_lane_mode=disabled`, `gate_enabled=false`; no `gate_enabled=true` | PASS | `runtime.log` lines near startup |
| Raw FSC capture off | `find logs/nln_capture -name '*r31...r3*'` | no final R31-r3 raw capture dir | PASS | shell check 2026-06-15T14:23Z |
| Lifecycle proof | lifecycle canary | no shadow/probe lifecycle rows in early polls | FAIL/OPEN | `lifecycle_canary/RUN_LIFECYCLE_CANARY_PROOF.md` |

Wniosek walidacyjny:
- R31 runtime jest uruchomiony w tmux i event path jest zdrowy.
- FSC storage/capture path zostal wylaczony dla finalnego R31-r3.
- Formalny selector lifecycle proof nie jest zamkniety; wymaga pozniejszego sprawdzenia albo osobnej decyzji, czy `program_streams` jest niezbedny dla probe lifecycle.

Ograniczenia walidacji:
- Nie czekano na pelny godzinny lifecycle launcher timeout, bo launcher zabilby tmux przy braku proof.
- Nie diagnozowano jeszcze, dlaczego probe selection nie przechodzi do probe transport przy `program_streams.enabled=false`.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: config fail-closed namespace
- Co zabezpiecza: nie miesza nowych probe artefaktow z abortowanymi startami.
- Kiedy sie aktywuje: `append=false` i istniejacy output.
- Jak przetestowano: preflight odmowil uzycia nieczystego R31 namespace.
- Co pozostaje poza zakresem: automatyczne sprzatanie abortowanych artefaktow.

Guardrail 2:
- Typ: runtime log check
- Co zabezpiecza: brak ukrytego FSC funding lane.
- Kiedy sie aktywuje: startup runtime.
- Jak przetestowano: grep runtime log dla `funding_lane_mode=disabled` i `gate_enabled=false`.
- Co pozostaje poza zakresem: przyszly kod moze dodac nowy writer poza tymi przelacznikami.

## Otwarte ryzyka / follow-up

- Sprawdzic, czy `program_streams.enabled=false` ogranicza probe lifecycle transport przez brak dodatkowego route/account evidence.
- Po powrocie uzytkownika wykonac status R31-r3 i policzyc statystyki runtime.
- Utrzymac obserwacje rozmiaru dysku, bo logi runtime nadal moga rosnac, mimo ze FSC raw capture jest off.
