# ADR-8D: FSC R28 provenance i timestamp lookup-audytu

Status: prepared
Typ: corrective / diagnostic integrity
Data: 2026-06-14
Repo/branch: /root/Gho, codex/gatekeeper-edge-policy-redesign-r1
Commit/PR: brak
Zakres: FSC v2 diagnostics, funding lane provenance, offline lookup audit
Dotkniete moduly/pliki: ghost-launcher/src/tx_intelligence/funding_source.rs; ghost-core/src/tx_intelligence/types.rs; ghost-brain/src/oracle/decision_logger.rs; scripts/fsc_attribution_lookup_audit.py; scripts/test_fsc_attribution_lookup_audit.py
Powiazane runy/logi/raporty: FSC_REP.md; R28 shadow-burnin-v3-r28-all-decision-counterfactual-30-30-maxwait4000
Poziom ryzyka: medium

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy: zweryfikowac FSC_REP/R28, odroznic realny niski clean coverage od martwej lane, naprawic tylko potwierdzone bledy diagnostyki/evidence.
Rzeczywisty przebieg: sprawdzono R28 jako zywy proces, artefakty raw full-chain, sidecar lookup candidates, konfiguracje R28 i aktywny kod FSC; wprowadzono waska naprawe provenance, lane warmup dla valid below-store transferow i buy-event timestamp.
Odchylenia od planu: nie restartowano R28, wiec nowy kod nie jest jeszcze runtime-proven na aktualnym procesie.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia: ochrona SSOT, shadow/live separation, replay/evidence semantics.
Zakres uzycia: klasyfikacja zmian jako diagnostic/evidence-only, bez zmiany Gatekeeper policy ani execution.
Wynik: utrzymano naprawe poza scoringiem, send path i config thresholds.
Ograniczenia: runtime proof wymaga nowego procesu po buildzie.

Nazwa: rust-master
Powod uzycia: Rust runtime state, blokady i testy regresyjne.
Zakres uzycia: zmiana stanu lane pod lockiem, serde-compatible pole Option, targeted cargo tests.
Wynik: testy targeted PASS.
Ograniczenia: workspace ma duzo istniejacych warningow niezwiązanych z ta zmiana.

Nazwa: solana-pumpfun-architect
Powod uzycia: rozroznienie full-chain Yellowstone lane od topicow NLN/system.
Zakres uzycia: provenance label dla authoritative_full_feed bez podszywania sie pod disabled system topic.
Wynik: provider/source_topics wskazuja grpc_funding_lane_full_chain dla full-chain lane.
Ograniczenia: nie zmieniano request-shape ani topic subscriptions.

## 3. Opis problemu - 3W2H

What: R28 raportowal niskie clean FSC coverage oraz legacy provider/source_topics mimo aktywnych raw full-chain transfer artifacts.
Where: FundingSourceIndex provenance/warmup, FscLookupDiagnostic, FSC lookup-audit script.
Why it matters: bez prawidlowego provenance i buy-event timestampu nie da sie odroznic realnych brakow direct funding od bledu evidence layer.
How observed: FSC_REP wskazywal glownie degraded/unavailable; sidecar pokazal duzo NO_RETAINED_RECIPIENT_HISTORY; raw funding_events mialy pelne full-chain dane; decyzje nadal raportowaly ghost_legacy_rolling_funding_index.
How many / scale: R28 mial tysiące decision rows i dziesiatki tysiecy lookup candidates; funding_events_v1 mial plik rzedu kilkunastu GB.
Evidence: live artifact sampling i targeted parser/join wykazaly raw full-chain lane jako aktywna, ale clean coverage pozostala niska przez semantyke direct buyer funding i progi.

## 4. Przyczyna zrodlowa

Root cause: evidence layer mial dwie luki diagnostyczne, a offline audit uzywal zbyt poznego timestampu.
Mechanizm bledu: transfer ponizej min_abs_store_lamports byl rejestrowany jako below-store i wracal przed aktualizacja saw_transfer/stream_available/provenance; fsc_v2_source_provenance rozpoznawal tylko nln_program_streams; sidecar nie niosl buy-event timestamp.
Miejsce: ghost-launcher/src/tx_intelligence/funding_source.rs; ghost-core/src/tx_intelligence/types.rs; ghost-brain/src/oracle/decision_logger.rs; scripts/fsc_attribution_lookup_audit.py.
Skutek: full-chain lane mogla wygladac jak cold/legacy w evidence, a offline audit mogl liczyc transfery po buy, ale przed decision_ts.
Dowod: test below_store_full_chain_transfer_marks_lane_warm_without_retained_history oraz sidecar test z buy_event_ts_ms.
Odrzucone hipotezy: nie potwierdzono martwej lane; raw full-chain funding_events istnieja. Nie potwierdzono potrzeby zmiany Gatekeeper policy, progow FSC ani TX buildera.

## 5. Strategia naprawy

Przyjeta strategia: minimalna korekta diagnostyczna i evidence-only.
Zakres ingerencji: lane state/provenance, serde-compatible diagnostic field, sidecar emission, streaming offline audit.
Czego nie zmieniano: Gatekeeper policy, scoring, hard reject, fsc thresholds, config rollout, Seer request-shape, shadow/live behavior, execution/send path.
Ryzyka: nowe pole sidecar jest additive; aktualny R28 proces nie ma tej zmiany do czasu restartu; true coverage nadal moze byc niska z powodow semantycznych.
Odrzucone alternatywy: obnizenie progow, traktowanie dust jako clean, rozszerzanie lookback bez walidacji, podszywanie full-chain lane pod disabled prod.rpc.solana.system.transfers.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: ghost-launcher/src/tx_intelligence/funding_source.rs
- Co zmieniono: valid transfer aktualizuje lane state/provenance przed decyzja store/drop; below-store full-chain transfer rozgrzewa lane, ale nie tworzy retained history.
- Dlaczego: lane availability i provenance sa evidence o strumieniu, nie tylko o retained attribution history.
- Efekt: below-store full-chain nie powoduje cold/legacy misreporting.

Zmiana 2:
- Plik/modul: ghost-launcher/src/tx_intelligence/funding_source.rs
- Co zmieniono: authoritative_full_feed mapuje sie na provider/source_topics grpc_funding_lane_full_chain.
- Dlaczego: R28 uzywa raw full-chain lane, nie legacy rolling index ani disabled system topic.
- Efekt: FSC v2 evidence rozroznia pelna lane od legacy/NLN.

Zmiana 3:
- Plik/modul: ghost-core/src/tx_intelligence/types.rs; ghost-brain/src/oracle/decision_logger.rs
- Co zmieniono: dodano FscLookupDiagnostic.buy_event_ts_ms i emisje w fsc_lookup_candidates_v1.
- Dlaczego: offline audit musi ciac okno przed buy event, nie przed terminal decision_ts.
- Efekt: additive schema field, backward compatible przez serde default.

Zmiana 4:
- Plik/modul: scripts/fsc_attribution_lookup_audit.py
- Co zmieniono: streaming funding events tylko dla lookup wallets, CSV/MD uwzglednia buy_event_ts_ms.
- Dlaczego: R28 funding_events ma kilkanascie GB i nie powinien byc ladowany w calosci do pamieci.
- Efekt: audyt jest wykonalny dla duzych plikow i nie liczy post-buy transferow jako pre-buy, gdy sidecar ma timestamp.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Python syntax | python3 -m py_compile scripts/fsc_attribution_lookup_audit.py scripts/test_fsc_attribution_lookup_audit.py | brak bledow | PASS | py_compile OK |
| Python unit | python3 -m unittest scripts/test_fsc_attribution_lookup_audit.py | 4 tests OK | PASS | output: Ran 4 tests OK |
| Rust targeted FSC | cargo test -p ghost-launcher below_store_full_chain_transfer_marks_lane_warm_without_retained_history -- --nocapture | 1 passed | PASS | targeted regression OK |
| Rust sidecar | cargo test -p ghost-brain test_fsc_lookup_candidate_sidecar_writes_lookup_wallet -- --nocapture | 1 passed | PASS | buy_event_ts_ms emitted |
| Rust FSC module | cargo test -p ghost-launcher tx_intelligence::funding_source::tests -- --nocapture | 43 passed | PASS | no funding_source regressions |

Wniosek walidacyjny: core code/test closure jest osiagniete dla naprawy evidence/provenance/offline audit.
Ograniczenia walidacji: nie wykonano runtime rerunu/canary na nowym binary; obecny R28 proces nie zostal zatrzymany ani restartowany.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: unit test
- Co zabezpiecza: below-store full-chain transfer rozgrzewa lane i zachowuje degraded semantics bez retained history.
- Kiedy sie aktywuje: przy regresji w observe_transfer dla transferow ponizej min_abs_store_lamports.
- Jak przetestowano: cargo test -p ghost-launcher below_store_full_chain_transfer_marks_lane_warm_without_retained_history.
- Co pozostaje poza zakresem: realny Solana runtime/canary.

Guardrail 2:
- Typ: unit test
- Co zabezpiecza: sidecar FSC emituje buy_event_ts_ms.
- Kiedy sie aktywuje: przy regresji w DecisionLogger sidecar schema.
- Jak przetestowano: cargo test -p ghost-brain test_fsc_lookup_candidate_sidecar_writes_lookup_wallet.
- Co pozostaje poza zakresem: pelny JSONL compatibility scan historycznych logow.

Guardrail 3:
- Typ: Python unit tests
- Co zabezpiecza: lookup audit uzywa buy_event_ts_ms i streamuje tylko relewantne funding events.
- Kiedy sie aktywuje: przy regresji offline audytu dla duzych R28 plikow.
- Jak przetestowano: python3 -m unittest scripts/test_fsc_attribution_lookup_audit.py.
- Co pozostaje poza zakresem: pelny przebieg na 15GB R28 funding_events w tym tasku.

## Otwarte ryzyka / follow-up

- Wymagany jest swiezy canary/rerun na nowym binary, aby formalnie zamknac runtime evidence dla provider/source_topics i buy_event_ts_ms.
- Niskie FSC clean coverage nie jest w pelni "naprawialne" kodowo bez zmiany semantyki/progow: duza czesc buyer wallets nie ma direct inbound SOL funding w oknie lub ma tylko dust/ponizej progow.
- Nie nalezy obnizac progow FSC ani wydluzac lookback jako cichej naprawy bez osobnej walidacji statystycznej i planu rollout.
