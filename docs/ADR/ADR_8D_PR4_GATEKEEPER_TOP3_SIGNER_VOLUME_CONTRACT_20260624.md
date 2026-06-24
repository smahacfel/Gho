# ADR-8D: PR4 Gatekeeper top3 signer-volume contract

Status: IMPLEMENTED / TARGETED_VALIDATION_COMPLETED
Typ: ADR-8D / gatekeeper whale concentration unit contract and JSONL compatibility
Data: 2026-06-24
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `6ad066c`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: implementacja PR4 z `PLAN_GATEKEEPER_V2_POLICY_SSOT_AND_AVAILABILITY_20260624.md`; doprecyzowanie semantyki top3/whale concentration, ratio-vs-percent boundary i addytywna kompatybilnosc JSONL
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-core/src/tx_intelligence/types.rs`
- `ghost-launcher/src/tx_intelligence/analysis.rs`
- `ghost-launcher/src/tx_intelligence/engine.rs`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/components/gatekeeper_adaptive_prosperity.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- test fixtures and regression tests for Gatekeeper/TxIntelligence compatibility

Powiazane plany:
- `PLANS/PLAN_GATEKEEPER_V2_POLICY_SSOT_AND_AVAILABILITY_20260624.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D zastosowany w `ADR_8D_PR1_GATEKEEPER_HARD_FAIL_SSOT_PARITY_20260624.md`, `ADR_8D_PR2_GATEKEEPER_PDD_SIGNAL_AVAILABILITY_20260624.md` i `ADR_8D_PR3_GATEKEEPER_CONFIDENCE_SEMANTICS_20260624.md`.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zrealizowac PR4 z planu Gatekeeper policy SSOT and availability, czyli usunac dwuznacznosc pola `top3_volume_pct`, jawnie rozdzielic ratio-scale `0.0..1.0` od percent-scale `0.0..100.0`, zachowac stare payloady i dodac regresje na semantyke signer-volume.

Rzeczywisty przebieg:
- Potwierdzono, ze aktywna sciezka TxIntelligence liczy koncentracje jako top-3 signer volume ratio.
- Dodano nowe pole `top3_signer_volume_ratio: Option<f64>` jako jawny, ratio-scale kontrakt.
- Zachowano stare `top3_volume_pct` jako ratio-scale compatibility alias.
- Dodano helper `effective_top3_signer_volume_ratio()` do czytania nowego pola z fallbackiem na stary alias.
- PDD nadal dostaje `whale_top3_pct` w percent-scale przez jawna konwersje `ratio * 100.0`.
- Nie zmieniono progow, BUY/REJECT, live/shadow behavior ani runtime alpha/ML integration.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: Gatekeeper evidence/policy boundary, SSOT decision snapshot, JSONL/replay compatibility.
- `rust-master`: serde compatibility, `Option<f64>` zamiast silent default zero, testy regresyjne i minimalny API helper.
- `trading-systems`: ryzyko jednostek ratio-vs-percent w filtrach ryzyka i diagnostyce whale concentration.

Zaladowane dokumenty specjalistyczne:
- `docs/agents/gatekeeper-policy-auditor.md`
- `docs/agents/ssot-feature-materialization-guardian.md`
- `docs/agents/decision-logging-replay-analyst.md`
- `docs/agents/config-rollout-safety-reviewer.md`

Powod:
PR4 dotyka aktywnej semantyki Gatekeeper/TxIntelligence oraz addytywnego shape JSONL. Nie dotyka Solana execution, live sendera, ingest parserow ani Yellowstone/Geyser routing.

## 3. Opis problemu - 3W2H

What:
Pole `top3_volume_pct` mialo mylaca nazwe i nieprecyzyjny kontrakt. W praktyce bylo ratio `0.0..1.0`, ale sufiks `pct` sugerowal percent-scale `0.0..100.0`. Dodatkowo nie bylo jasne, czy metryka oznacza signer-volume, wallet-volume, entity-volume czy tx-count concentration.

Where:
- `TxIntelFeatures`
- `SignerDiversityProfile`
- Gatekeeper diversity/hard-fail/policy paths
- PDD `whale_top3_pct`
- `GatekeeperBuyLog`
- regression fixtures and replay-compatible payloads

Why it matters:
W Gatekeeperze pomylenie ratio z procentem moze zmienic skutecznosc whale concentration filters bez jawnej zmiany configu. Rownie niebezpieczne jest dodanie nowego `f64` z `#[serde(default)]`, bo stare payloady bez pola dostalyby `0.0`, czyli brak pola bylby nierozroznialny od prawdziwego zera.

How observed:
Plan PR4 wymagal wybrania SSOT semantics zamiast numeric parity testu. Biezacy kod materializowal signer-volume ratio, ale nazwa i czesc downstream field names nie wymuszaly tego kontraktu.

How many / scale:
Zmiana jest addytywna i lokalna dla top3 concentration. Obejmuje core type, launcher materialization/policy, buy-log export i testy. Nie zmienia config threshold values, live mode ani decision triggerow.

## 4. Przyczyna zrodlowa

Root cause:
Historyczny alias `top3_volume_pct` laczyl trzy rozne rzeczy:
- compatibility field name,
- ratio-scale value,
- implicit signer-volume interpretation.

Brak jawnego pola `top3_signer_volume_ratio` oraz brak helpera fallback powodowaly, ze kolejne miejsca mogly czytac stary alias bez swiadomosci jednostki i semantyki.

## 5. Strategia naprawy

Przyjeta strategia:
- Dodac nowe opcjonalne pole `top3_signer_volume_ratio`.
- Nie stosowac `#[serde(default)] f64` dla nowego pola; brak pola ma pozostac rozroznialny.
- Zachowac `top3_volume_pct` jako stary ratio-scale alias.
- Dodac helper `effective_top3_signer_volume_ratio()` jako jedyny bezpieczny sposob odczytu w aktywnych sciezkach.
- W nowych payloadach ustawic oba pola: nowe canonical pole oraz stary alias.
- W starych payloadach fallbackowac do `top3_volume_pct`, bez silent zero dla nowego pola.
- Jawnie konwertowac ratio na PDD percent tylko na boundary PDD.

Granice:
- Brak zmian progow.
- Brak live enablement.
- Brak nowego BUY triggera.
- Brak runtime hooka XGBoost/31100.
- Brak walidacji ML w PR4.
- Brak destrukcyjnego usuwania starych JSONL fields.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: core contract
- Plik: `ghost-core/src/tx_intelligence/types.rs`
- Dodano `top3_signer_volume_ratio: Option<f64>`.
- Zachowano `top3_volume_pct: f64` jako compatibility alias.
- Dodano `TxIntelFeatures::effective_top3_signer_volume_ratio()`.

Zmiana 2: launcher TxIntelligence materialization
- Pliki:
  - `ghost-launcher/src/tx_intelligence/analysis.rs`
  - `ghost-launcher/src/tx_intelligence/engine.rs`
- `SignerDiversityProfile` dostal nowe optional canonical pole i helper fallback.
- Nowe feature payloady materializuja `top3_signer_volume_ratio = Some(ratio)` i `top3_volume_pct = ratio`.
- Risk flag diagnostics mowia o `top3_signer_volume_ratio`.

Zmiana 3: Gatekeeper active policy paths
- Pliki:
  - `ghost-launcher/src/components/gatekeeper_policy.rs`
  - `ghost-launcher/src/components/gatekeeper.rs`
- Diversity soft score, hard-fail checks, phase checks i long-phase checks czytaja helper `effective_top3_signer_volume_ratio()`.
- PDD `whale_top3_pct` dostaje jawna konwersje `ratio * 100.0` z komentarzem o granicy jednostek.

Zmiana 4: buy-log JSONL compatibility
- Plik: `ghost-brain/src/oracle/decision_logger.rs`
- `GATEKEEPER_BUY_LOG_SCHEMA_VERSION` zwiekszono do `33`.
- Dodano opcjonalne `top3_signer_volume_ratio`.
- Zachowano stare `top3_volume_pct` jako ratio-scale alias.
- Dodano test, ze write path emituje oba pola.

Zmiana 5: fixtures and regression tests
- Zaktualizowano fixtures, ktore materializuja `TxIntelFeatures` albo `SignerDiversityProfile`.
- Dodano test core serde/fallback dla nowego i starego payloadu.
- Dodano test TxIntelligence potwierdzajacy signer-volume semantics, a nie tx-count concentration.
- Dodano test Gatekeeper PDD scale: `0.60` ratio materializuje sie jako `60.0` percent.
- Dodano regression test, ze nowe pole wygrywa nad legacy aliasem, a legacy alias nadal dziala przy braku nowego pola.

## 7. Walidacja dzialan naprawczych

Targeted validation wykonana na aktualnym checkoutcie:

```bash
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_policy_tests top3 -- --nocapture
# result: 3 passed

CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-core tx_intelligence -- --nocapture
# result: 2 tx_intelligence_contract_tests passed

CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher tx_intelligence -- --nocapture
# result: 77 lib tests passed; 1 refactor invariant test passed; 2 tx_intelligence integration tests passed

CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_v25_regression top3 -- --nocapture
# result: 1 passed

CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-brain gatekeeper_buy_log -- --nocapture
# result: 3 GatekeeperBuyLog serialization/deserialization/write tests passed
```

Statyczny diff/scale audit:
- `cargo fmt --all -- --check` - PASS.
- `git diff --check -- ghost-core/src/tx_intelligence ghost-core/tests/tx_intelligence_contract_tests.rs ghost-core/tests/checkpoint_engine_tests.rs ghost-core/tests/feature_builder_tests.rs ghost-core/tests/pr1_contracts_foundations.rs ghost-launcher/src/tx_intelligence ghost-launcher/src/components ghost-launcher/src/oracle_runtime.rs ghost-launcher/tests/gatekeeper_policy_tests.rs ghost-launcher/tests/gatekeeper_v25_regression.rs ghost-launcher/tests/tx_intelligence_tests.rs ghost-brain/src/oracle/decision_logger.rs docs/ADR` - PASS.
- `rg -n "top3_volume_pct \* 100|tx_intel_features\.top3_volume_pct \* 100|\.top3_volume_pct \* 100" ghost-core/src ghost-launcher/src ghost-brain/src/oracle/decision_logger.rs` - no matches.
- `rg -n "pub top3_signer_volume_ratio: f64|top3_signer_volume_ratio: f64|serde\(default\).*top3_signer_volume_ratio" ghost-core/src ghost-launcher/src ghost-brain/src/oracle/decision_logger.rs ghost-core/tests ghost-launcher/tests` - no matches.
- `rg -n "[ \t]$" docs/ADR/ADR_8D_PR4_GATEKEEPER_TOP3_SIGNER_VOLUME_CONTRACT_20260624.md ghost-core/tests/tx_intelligence_contract_tests.rs` - no matches for untracked new files.

## 8. Co zostalo potwierdzone

- Nowe payloady maja jawne `top3_signer_volume_ratio`.
- Stare payloady bez nowego pola fallbackuja do `top3_volume_pct`.
- Brak nowego pola nie materializuje sie automatycznie jako `0.0`.
- `top3_volume_pct` zostaje w JSONL jako ratio-scale compatibility alias.
- PDD `whale_top3_pct` pozostaje percent-scale i dostaje tylko jawnie skonwertowana wartosc.
- Aktywne sciezki Gatekeeper V2/V2.5 korzystaja z helpera zamiast czytac stary alias jako nowe zrodlo prawdy.

## 9. Ryzyka resztkowe / czego PR4 jeszcze nie robi

- PR4 nie usuwa starego `top3_volume_pct`; pole pozostaje wymagane dla kompatybilnosci payloadow i analiz historycznych.
- Nazwy w niektorych zewnetrznych raportach/skryptach moga nadal uzywac starego aliasu; to nie jest aktywny runtime policy path.
- PR4 nie normalizuje koncentracji po wallet/entity; obecny kontrakt jest signer-volume ratio.
- PR4 nie wprowadza `alpha_31100_candidate_v1` do runtime.
- PR4 nie waliduje modelu ML; to osobny future validation harness po zakonczeniu planu.

## 10. Scope out

Poza zakresem PR4:
- deploy live,
- nowy BUY trigger,
- XGBoost/runtime alpha score,
- threshold tuning,
- globalna migracja wszystkich historycznych nazw `top3_volume_pct`,
- destrukcyjna migracja JSONL,
- przepisywanie Segment Lab lub raportow HTML do bota.

## 11. Decyzja koncowa

PR4 zamyka kontrakt whale top3 concentration w aktywnej sciezce Gatekeeper V2/V2.5:
- canonical nazwa: `top3_signer_volume_ratio`,
- canonical scale: ratio `0.0..1.0`,
- compatibility alias: `top3_volume_pct`,
- PDD boundary scale: `whale_top3_pct` percent `0.0..100.0`,
- fallback: helper `effective_top3_signer_volume_ratio()`.

Zmiana jest addytywna, replay-compatible i nie zmienia polityki BUY/REJECT poza usunieciem ryzyka niejawnej interpretacji jednostek.
