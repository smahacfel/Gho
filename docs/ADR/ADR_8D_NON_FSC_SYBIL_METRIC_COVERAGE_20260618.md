# ADR-8D: Non-FSC sybil metric coverage hardening for R35

Status: IMPLEMENTED / RUNTIME_PROOF_COLLECTED
Typ: ADR-8D / runtime feature materialization coverage guard
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `main`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR update time
Zakres: Gatekeeper V2/V2.5 sybil metric materialization for FTDI, DBIA, SFD, DES and CPV review; FSC explicitly out of scope
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-launcher/src/tx_intelligence/sybil_metrics.rs`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/tests/session_lifecycle_tests.rs`
- `off-chain/components/trigger/src/direct_buy_builder.rs`
- `off-chain/components/trigger/src/lib.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- `configs/rollout/shadow-burnin-v3-r35-threshold-probe-target50-stop50-fsc-off-r1.toml`
- `configs/rollout/ghost_brain_selector_dataset_sampler_r35_threshold_probe_maxwait3789_fsc_off.toml`

Powiazane runy/logi/raporty:
- `logs/rollout/shadow-burnin-v3-r35-threshold-probe-target50-stop50-fsc-off-r1/`
- `logs/shadow_run/shadow-burnin-v3-r35-threshold-probe-target50-stop50-fsc-off-r1/`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje istniejacy lokalny format ADR-8D uzyty juz w repo.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Poprawic coverage anti-sybil/anycabal z R35 dla metryk FTDI, DBIA, SFD, DES i CPV, bez dotykania FSC. Zachowac ostroznosc wobec shadow simulation/runtime i, jesli restart bedzie wymagany, uruchomic R35 ponownie na tej samej konfiguracji.

Rzeczywisty przebieg:
- Potwierdzono aktywny runtime path: `PoolObservationSession::materialize_features()` -> `compute_sybil_resistance()` -> `MaterializedFeatureSet.sybil_resistance` -> Gatekeeper V2/V2.5 diagnostics/logger.
- FSC pozostalo poza zakresem.
- CPV zostalo przeanalizowane, ale nie zmienione: aktualna semantyka fail-closed dla `<3` signerow i cold rolling state jest poprawna.
- Naprawiono materializacje FTDI, DBIA, SFD i DES tak, aby emitowaly wartosci diagnostyczne tam, gdzie istnieje minimalna policzalna probka.
- Dodano policy guard test: low-sample values pozostaja non-actionable w Gatekeeper scoring.
- W trakcie walidacji ujawnil sie niezalezny build/runtime blocker w legacy buy protocol tail i role mapping. Zostal naprawiony, bo blokowal release build i runtime proof R35.
- Odtworzono brakujace configi R35 z poprzedniego commita i zrestartowano R35 na tej samej konfiguracji po release buildzie.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: ochrona SSOT, shadow/live boundary, typed degraded reasons i Gatekeeper policy semantics.
- `rust-master`: lokalna implementacja Rust, testy, brak nowych globalnych stanow, brak blokujacych operacji hot-path.
- `solana-pumpfun-architect`: naprawa legacy buy protocol tail i `breaking_fee_recipient` role mapping wymagana do release/runtime validation.

Zaladowane dokumenty specjalistyczne:
- `docs/agents/solana-execution-path-engineer.md`
- `docs/agents/ssot-feature-materialization-guardian.md`
- `docs/agents/gatekeeper-policy-auditor.md`

Powod:
Zadanie zaczelo sie jako feature materialization/policy coverage, ale walidacja release i shadow runtime dotknela Solana execution handoff. Specialist docs zostaly zaladowane tylko dla kontraktow faktycznie zagrozonych przez zmiane i build blocker.

## 3. Opis problemu - 3W2H

What:
R35 pokazywal zbyt czeste `None` albo degraded-only output dla non-FSC sybil metrics: FTDI, DBIA, SFD, DES i CPV.

Where:
Aktywna sciezka:
`PoolObservationSession::materialize_features()` -> `compute_sybil_resistance()` -> `MaterializedFeatureSet.sybil_resistance` -> `build_sybil_policy_diagnostics()` -> `gatekeeper_v2_decisions.jsonl` / `gatekeeper_v2_buys.jsonl`.

Why it matters:
R35 jest runem diagnostycznym. Brak wartosci utrudnia rozroznienie:
- rzeczywistego braku sygnalu,
- braku surowych danych,
- sygnalu policzalnego, ale z probka zbyt slaba do scoringu.

How observed:
Historyczne i biezace logi R35 zawieraly powtarzalne reasons:
- `FTDI_INSUFFICIENT_BUYS`
- `DBIA_INSUFFICIENT_BUYERS`
- `SFD_INSUFFICIENT_BUYS`
- `SFD_POSTBALANCE_UNAVAILABLE`
- `DES_INSUFFICIENT_BUYS`
- `DES_CURVE_DATA_UNAVAILABLE`
- `CPV_INSUFFICIENT_SIGNERS` / cold rolling state cases

How many / scale:
Zmiana dotyczy kazdej materializacji sybil metrics w aktywnym V2/V2.5 path, ale tylko jako feature value/degraded reason. Nie zmieniono progow Gatekeeper, FSC, hard fails, live execution ani JSONL schema.

## 4. Przyczyna zrodlowa

Root cause:
FTDI, DBIA, SFD i DES traktowaly czesc low-sample/partial-evidence przypadkow jako calkowity brak wartosci, mimo ze minimalna wartosc diagnostyczna byla policzalna i mogla pozostac oznaczona degraded reason.

Mechanizm:
- FTDI wymagalo clean sample count, zamiast rozdzielic diagnostic value od clean/actionable value.
- DBIA nie emitowalo wartosci dla dev + jeden non-dev buyer.
- SFD tracilo wynik przy braku postbalance, nawet gdy znany byl buy amount.
- DES tracilo wynik przy brakujacej curve reserve, mimo dostepnego `price_quote` fallback.
- CPV bylo mylone z metryka do "safe zero"; w aktualnym runtime `<3` signerow nadal musi pozostac `None`, bo zero udawaloby brak ryzyka bez probki.

Odrzucone hipotezy:
- Ustawic CPV na `Some(0.0)` przy `<3` signerach: odrzucone jako ukrycie braku probki.
- Zmieniac FSC/funding lane: odrzucone przez scope.
- Usuwac degraded reasons przy low sample: odrzucone, bo low-sample values nie moga stac sie actionable.

## 5. Strategia naprawy

Przyjeta strategia:
Emitowac wartosci diagnostyczne, gdy istnieje minimalna policzalna probka, ale zachowac degraded reason, ktory blokuje actionability w policy scoring.

Granice:
- Raw missing albo cold state nadal daje `None`.
- Low-sample diagnostic value ma byc widoczna w evidence, ale nie moze naliczac soft points.
- JSONL schema pozostaje bez zmiany.
- FSC pozostaje nietkniete.
- Shadow/live boundary pozostaje bez zmiany.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: FTDI
- Plik: `ghost-launcher/src/tx_intelligence/sybil_metrics.rs`
- Dodano minimalny prog diagnostyczny dla dwoch unikalnych buy samples.
- Wartosc jest emitowana z `FTDI_INSUFFICIENT_BUYS`, gdy probka nie jest clean.
- Raw topology missing nadal pozostaje `None`.

Zmiana 2: DBIA
- Plik: `ghost-launcher/src/tx_intelligence/sybil_metrics.rs`
- Dev + jeden non-dev buyer moze wyemitowac wartosc diagnostyczna.
- Wartosc jest emitowana z `DBIA_INSUFFICIENT_BUYERS`, gdy probka nie jest clean.
- Missing dev buy albo raw fingerprint missing nadal pozostaje fail-closed.

Zmiana 3: SFD
- Plik: `ghost-launcher/src/tx_intelligence/sybil_metrics.rs`
- Dodano buy amount fallback przez `sol_amount_lamports` albo dodatnie `volume_sol`, gdy postbalance jest niedostepny.
- SFD nadal wymaga prebalance denominator i wystarczajacej probki signerow.
- Partial coverage pozostaje oznaczane przez degraded reason.

Zmiana 4: DES
- Plik: `ghost-launcher/src/tx_intelligence/sybil_metrics.rs`
- `curve_price()` uzywa finite positive curve reserve price, a gdy go brak, finite positive `price_quote`.
- DES dopuszcza diagnostyczny wynik z minimalnych porownywalnych par.
- Brak wystarczajacych danych curve/slot nadal pozostaje fail-closed.

Zmiana 5: policy guard
- Plik: `ghost-launcher/src/components/gatekeeper_policy.rs`
- Dodano `insufficient_sample_sybil_values_remain_non_actionable`.
- Test potwierdza, ze low-sample FTDI/DBIA/DES values nie naliczaja policy points.

Zmiana 6: session tests
- Plik: `ghost-launcher/tests/session_lifecycle_tests.rs`
- Zaktualizowano oczekiwane degraded reasons i fixture partial SFD.
- Dodano sprawdzenie `buy_sample_count` i `SFD_PARTIAL_BALANCE_COVERAGE_REASON`.

Zmiana 7: release/build blocker w legacy buy tail
- Pliki: `off-chain/components/trigger/src/direct_buy_builder.rs`, `off-chain/components/trigger/src/lib.rs`
- Przywrocono publiczny BCV2/legacy tail contract: `BREAKING_FEE_RECIPIENTS`, `BreakingFeeRecipientStrategy`, `LegacyBondingCurveTailResolver`.
- Legacy buy branch buduje protocol-derived BCV2 + first static breaking fee recipient tail zamiast zaleznego observed tail.

Zmiana 8: runtime shadow role mapping
- Plik: `ghost-launcher/src/components/trigger/component.rs`
- Legacy account index 16 mapuje sie teraz na `bonding_curve_v2`.
- Legacy account index 17 mapuje sie teraz na `breaking_fee_recipient`.
- Required-account probe traktuje `bonding_curve_v2` jako tail index 0 i `breaking_fee_recipient` jako tail index 1.
- To usuwa runtime mismatch `selected_legacy_buy_final_manifest_missing_breaking_fee_recipient`.

Zmiana 9: config recovery
- Pliki:
  - `configs/rollout/shadow-burnin-v3-r35-threshold-probe-target50-stop50-fsc-off-r1.toml`
  - `configs/rollout/ghost_brain_selector_dataset_sampler_r35_threshold_probe_maxwait3789_fsc_off.toml`
- Odtworzono brakujace configi R35 z commita `fa56680b19e005b051be8109a38fe77014d1b23f`, bo bez nich restart launcher probowal uzyc placeholder `localhost:10000`.

## 7. Walidacja dzialan naprawczych

### Static/build/test validation

| Walidacja | Komenda/run | Wynik | Status |
|---|---|---|---|
| Sybil session targeted | `cargo test -q -p ghost-launcher --test session_lifecycle_tests materialize_features_keeps_sfd_when_partial_balance_coverage_still_has_three_usable_signers -- --nocapture` | passed | PASS |
| Session sybil population | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher --test session_lifecycle_tests materialize_features_populates_ -- --nocapture` | 6 tests passed | PASS |
| Sybil metrics unit set | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher tx_intelligence::sybil_metrics -- --nocapture` | 22 tests passed | PASS |
| Policy non-actionable guard | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher insufficient_sample_sybil_values_remain_non_actionable -- --nocapture` | 1 test passed | PASS |
| Trigger direct buy tests | `RUSTFLAGS=-Awarnings cargo test -q -p trigger direct_buy_builder -- --nocapture` | 23 tests passed | PASS |
| P37 required accounts | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher p37_counterfactual_probe_required_accounts -- --nocapture` | 6 tests passed | PASS |
| P37 protocol legacy probes | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher p37_shadow_probe_protocol_legacy -- --nocapture` | 4 tests passed | PASS |
| Selected fallback handoff | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher selected_fallback_handoff_canonicalizes_protocol_bcv2_tail_after_prepare -- --nocapture` | 1 test passed | PASS |
| E5A prepared manifest | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher e5a_prepared_legacy_buy_final_manifest_with_observed_remaining_accounts_uses_protocol_tail -- --nocapture` | 1 test passed | PASS |
| Legacy precheck manifest | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher selected_legacy_buy_precheck_manifest_uses_protocol_bcv2_tail -- --nocapture` | 1 test passed | PASS |
| Legacy simulation manifest | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher selected_legacy_buy_simulation_manifest_uses_protocol_bcv2_tail -- --nocapture` | 1 test passed | PASS |
| Rustfmt touched files | `rustfmt --edition 2021 --check ...` | passed | PASS |
| Release build | `RUSTFLAGS=-Awarnings cargo build -q -p ghost-launcher --release` | passed | PASS |
| Release binary | `stat target/release/ghost-launcher` | size `31744416`, mtime `2026-06-18 04:22:04 +0000` | PASS |

### R35 restart validation

R35 command:

```text
target/release/ghost-launcher --config configs/rollout/shadow-burnin-v3-r35-threshold-probe-target50-stop50-fsc-off-r1.toml
```

Restart facts:
- tmux session: `r35-threshold-probe-target50-stop50`
- latest restart: `2026-06-18T04:22:46Z`
- process after restart: `3030222 target/release/ghost-launcher --config configs/rollout/shadow-burnin-v3-r35-threshold-probe-target50-stop50-fsc-off-r1.toml`
- stdout scan after restart: 141346 timestamped lines through `2026-06-18T04:30:03.959110Z`
- `CONFIG VALIDATION FAILED`: 0 after restart
- process panic/panicked/backtrace/OOM: 0 after restart
- `selected_legacy_buy_final_manifest_missing_breaking_fee_recipient`: 0 after restart
- `selected_route_handoff_mismatch`: 0 after restart

Notes:
- The word `panic` appeared only in normal SignalRouter summary fields like `panic=0`.
- The word `fatal` appeared only in startup config text `fatal_exit=true`.

### Runtime sybil coverage proof after latest restart

Window:
- Start boundary: `2026-06-18T04:22:46+00:00`
- Decisions range: `2026-06-18T04:22:50.832770+00:00` -> `2026-06-18T04:29:59.905560+00:00`
- Buys range: `2026-06-18T04:22:51.231261+00:00` -> `2026-06-18T04:29:54.172299+00:00`

`gatekeeper_v2_decisions.jsonl`, rows after restart: 127

| Metric | Present | Eligible present | Main remaining missing reason |
|---|---:|---:|---|
| FTDI | 69/127, 54.3% | 69/69, 100.0% | `FTDI_INSUFFICIENT_BUYS` |
| DBIA | 69/127, 54.3% | 69/69, 100.0% | `DBIA_INSUFFICIENT_BUYERS`, `DBIA_NO_DEV_BUY` |
| SFD | 40/127, 31.5% | 40/40, 100.0% | `SFD_INSUFFICIENT_BUYS`, `SFD_POSTBALANCE_UNAVAILABLE` |
| DES | 49/127, 38.6% | 49/49, 100.0% | `DES_INSUFFICIENT_BUYS` |
| CPV | 31/127, 24.4% | 31/31, 100.0% | `CPV_INSUFFICIENT_SIGNERS` |

`gatekeeper_v2_buys.jsonl`, rows after restart: 46

| Metric | Present | Eligible present | Main remaining missing reason |
|---|---:|---:|---|
| FTDI | 46/46, 100.0% | 46/46, 100.0% | low-sample values retained as degraded |
| DBIA | 46/46, 100.0% | 46/46, 100.0% | low-sample values retained as degraded |
| SFD | 25/46, 54.3% | 25/25, 100.0% | `SFD_POSTBALANCE_UNAVAILABLE`, `SFD_INSUFFICIENT_BUYS` |
| DES | 40/46, 87.0% | 40/40, 100.0% | `DES_INSUFFICIENT_BUYS` |
| CPV | 24/46, 52.2% | 24/24, 100.0% | `CPV_INSUFFICIENT_SIGNERS` |

Interpretacja:
- Dla kazdej non-FSC metryki, gdy probka jest bezpiecznie eligible wedlug aktualnego kontraktu, value coverage wynosi 100%.
- Pozostale `None` sa nadal fail-closed i wynikaja z braku minimalnej probki, braku dev buy/raw fingerprint, braku SFD denominator albo `<3` signerow dla CPV.
- FTDI/DBIA/DES low-sample diagnostic values sa widoczne, ale pozostaja non-actionable przez degraded reasons.

### Runtime shadow handoff proof after latest restart

Line boundaries przed restartem:
- `shadow_lifecycle.jsonl`: 64
- `probe_transport.jsonl`: 101
- `shadow_entries.jsonl`: 47
- `probe_selection.jsonl`: 111

Nowe artefakty po restarcie:
- `shadow_lifecycle.jsonl`: lines 65-107, 43 new rows
- `probe_transport.jsonl`: lines 102-109, 8 new rows
- `shadow_entries.jsonl`: lines 48-64, 17 new rows
- `probe_selection.jsonl`: lines 112-119, 8 new rows

Handoff proof:
- `selected_route_handoff_status=selected_route_handoff_applied`: 8 in transport, 2 in lifecycle/entries
- `selected_route_handoff_reason=selected_legacy_buy_final_manifest_validated`: 8 in transport, 2 in lifecycle/entries
- hits `selected_legacy_buy_final_manifest_missing_breaking_fee_recipient`: 0
- hits `selected_route_handoff_mismatch`: 0
- lifecycle roles include both:
  - `bonding_curve_v2:...:route_builder`
  - `breaking_fee_recipient:5YxQFdt3Tr9zJLvkFccqXVUwhdTWJQc1fFg2YPbxvxeD:route_builder`

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1: policy actionability
- Low-sample FTDI/DBIA/DES values nie naliczaja policy points.
- Test: `insufficient_sample_sybil_values_remain_non_actionable`.

Guardrail 2: raw evidence fail-closed
- FTDI raw topology missing i DBIA raw fingerprint missing nadal daja `None`.
- Nie wprowadzono synthetic zero ani silent fallback dla raw-missing.

Guardrail 3: SFD denominator safety
- SFD fallback uzywa buy amount tylko tam, gdzie nadal istnieje bezpieczny denominator.
- Brak prebalance/usable denominator nadal blokuje materializacje.

Guardrail 4: CPV no-change
- CPV pozostaje `None` przy `<3` signerach albo cold rolling state.
- Nie wprowadzono `Some(0.0)` jako ukrytego "braku ryzyka".

Guardrail 5: shadow/live boundary
- R35 pozostaje shadow/probe evidence path.
- Submit nadal nie jest traktowany jako confirmation.
- Unknown execution status nie jest traktowany jako success.

Guardrail 6: legacy buy tail contract
- Protocol-derived BCV2 + breaking fee recipient tail jest testowany na builderze, precheck, simulation manifest i runtime role mapping.

## 9. Pozostale ryzyka i ograniczenia

- Runtime proof jest smoke-window po restarcie, nie formalnym zamknieciem calego target50/stop50 rollout.
- SFD nadal bedzie mial `None`, gdy brakuje denominator albo minimalnej probki signerow; to jest oczekiwane fail-closed.
- CPV nadal bedzie mial `None` przy `<3` signerach; to jest oczekiwane fail-closed.
- FSC pozostaje poza zakresem i nadal moze dominowac degraded reasons w innych raportach.
- Nie wykonano commita ani push.

## 10. Decyzja koncowa

Zadanie non-FSC coverage dla FTDI, DBIA, SFD, DES i CPV mozna uznac za domkniete na poziomie kodu, targeted tests, release build i swiezego runtime smoke proof R35.

Core code status:
- CLOSED dla non-FSC metric materialization i policy safety guard.

Runtime smoke status:
- CLOSED dla post-restart evidence window po `2026-06-18T04:22:46Z`.

Formal rollout status:
- NOT CLOSED jako pelny rollout target50/stop50; R35 nadal powinien kontynuowac zbieranie artefaktow, jezeli wymagany jest dluzszy raport rolloutowy.
