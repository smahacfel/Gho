# ADR-8D: PR3 Gatekeeper confidence semantics

Status: IMPLEMENTED / TARGETED_VALIDATION_COMPLETED
Typ: ADR-8D / gatekeeper confidence availability and JSONL compatibility
Data: 2026-06-24
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `6ad066c`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: implementacja PR3 z `PLAN_GATEKEEPER_V2_POLICY_SSOT_AND_AVAILABILITY_20260624.md`; semantyka canonical V2.5 confidence dla alpha disabled/unavailable oraz separacja canonical confidence od uproszczonych shadow-stage scores
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- `PLANS/BACKLOG_ALPHA_31100_CANDIDATE_V1_VALIDATION_HARNESS_20260624.md`

Powiazane plany:
- `PLANS/PLAN_GATEKEEPER_V2_POLICY_SSOT_AND_AVAILABILITY_20260624.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D zastosowany w `ADR_8D_PR1_GATEKEEPER_HARD_FAIL_SSOT_PARITY_20260624.md` i `ADR_8D_PR2_GATEKEEPER_PDD_SIGNAL_AVAILABILITY_20260624.md`.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zrealizowac PR3 z planu Gatekeeper policy SSOT and availability bez zmiany progow decyzyjnych, bez enablementu live, bez nowego BUY triggera i bez wprowadzania kandydata ML `alpha_31100_candidate_v1` do runtime decision.

Rzeczywisty przebieg:
- Potwierdzono, ze PR3 dotyczy canonical V2.5 confidence, buy-log diagnostics oraz temporal decision snapshot.
- Zachowano multiplikatywny model confidence.
- Dla `enable_alpha_gate=false` alpha jest neutralnym komponentem wzoru, ale logi odrozniaja to od realnej zmierzonej jakosci alpha.
- Dla `enable_alpha_gate=true` oraz alpha skipped/not-run/missing canonical confidence jest unavailable z reason, a nie `0.0`.
- Oddzielono canonical `assessment.v25_confidence` od lokalnego Early/Normal shadow-stage score.
- Podczas walidacji confidence regression wykryto, ze deadline/sweep clock nadpisywal event-time evidence (`highest_seen_ts`) i mogl rozjechac Extended timer vs deadline fallback confidence. Naprawa zachowuje wall-clock jako zegar deadline, ale nie nadpisuje event-time evidence, gdy transakcje juz istnieja.
- Nie dodano zadnego runtime hooka XGBoost/31100.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: Gatekeeper policy boundary, DecisionLogger/JSONL auditability, shadow/live separation.
- `rust-master`: lokalny typ Rust, kontrola `Option`/availability semantics, testy regresyjne.
- `trading-systems`: ryzyko mylenia disabled component, unavailable evidence i realnej jakosci sygnalu.
- `statistical-research-engine`: klasyfikacja `alpha_31100_candidate_v1` jako research/shadow-only backlog, bez runtime decision.

Zaladowane dokumenty specjalistyczne:
- `docs/agents/gatekeeper-policy-auditor.md`
- `docs/agents/decision-logging-replay-analyst.md`
- `docs/agents/config-rollout-safety-reviewer.md`

Powod:
PR3 zmienia semantyke availability dla confidence i addytywny shape buy-log JSONL. Nie dotyka Solana execution, ingest parserow, Seer/Yellowstone ani runtime sendera, wiec te specialist docs nie byly potrzebne.

## 3. Opis problemu - 3W2H

What:
Canonical V2.5 confidence moglo mieszac trzy semantycznie rozne stany alpha:
- disabled alpha jako neutralny element wzoru,
- realnie dostepna alpha quality,
- enabled alpha, ktora nie zostala uruchomiona, zostala pominieta albo nie miala wymaganych inputow.

Where:
- `GatekeeperAssessment::v25_confidence_breakdown()`
- `GatekeeperAssessment::v25_confidence_availability()`
- shadow Early/Normal decision evaluation w `gatekeeper.rs`
- `GatekeeperBuyLog` w `ghost-brain/src/oracle/decision_logger.rs`

Why it matters:
W gatekeeperze `0.0` oznacza bardzo slaby komponent, a nie brak evidence. Jesli missing/skipped alpha materializuje sie jako `0.0`, confidence zostaje zanizone bez jawnej przyczyny. Jesli disabled alpha raportuje sie tak samo jak realna jakosc `1.0`, logi i replay moga mylnie sugerowac, ze alpha miala perfekcyjna jakosc. Dodatkowo uproszczony Early/Normal shadow score nie moze nadpisywac canonical V2.5 confidence.

How observed:
Plan PR3 wskazal, ze canonical path mial usunac `unwrap_or(0.0)` dla alpha metrics, dodac neutral-disabled vs unavailable distinction i zabezpieczyc Early/Normal przed publikowaniem uproszczonego score jako canonical `v25_confidence`.

How many / scale:
Zmiana jest lokalna do Gatekeeper assessment/logging. Nie zmienia thresholdow, typed verdictow, BUY/REJECT, live execution ani aktywnych hard-fail rules.

## 4. Przyczyna zrodlowa

Root cause:
Historyczny model confidence mial tylko numeric component values i opcjonalne fieldy diagnostyczne. Brakowalo lokalnego typu rozrozniajacego:
- neutral disabled component,
- available measured component,
- unavailable component with reason.

W efekcie `Option<f64>` dla alpha quality bylo zbyt ubogie jako kontrakt, a helpery mogly latwo mylic missing data z wynikiem `0.0`.

## 5. Strategia naprawy

Przyjeta strategia:
- Dodac lokalny enum `AlphaConfidenceInput`.
- Traktowac disabled alpha jako neutralny mnoznik `1.0`, ale emitowac status `neutral_disabled`.
- Traktowac enabled-but-skipped/missing/not-run jako `Unavailable { reason }`.
- Zwrocic `None` z canonical `v25_confidence_breakdown()` przy unavailable alpha.
- Rozszerzyc `v25_confidence_availability()` o alpha unavailable reasons.
- Dodac addytywne JSONL pola status/reason, z zachowaniem kompatybilnosci serde.
- Zastapic cacheowanie lokalnego shadow-stage score przez cacheowanie canonical confidence tylko wtedy, gdy jest dostepne.

Granice:
- Brak zmian thresholdow.
- Brak live enablement.
- Brak nowego BUY triggera.
- Brak runtime uzycia `alpha_31100_candidate_v1`.
- Brak walidacji ML w PR3.
- Brak kopiowania regul z raportow HTML.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: lokalny model alpha confidence input
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- Dodano `AlphaConfidenceInput::{NeutralDisabled, Available, Unavailable}`.
- Dodano stabilne status labels:
  - `neutral_disabled`,
  - `available`,
  - `unavailable`.
- Dodano reasons:
  - `alpha_not_run`,
  - `alpha_insufficient_sample`,
  - `alpha_missing_inputs`,
  - albo istniejacy stabilny `skip_reason`.

Zmiana 2: canonical V2.5 confidence rozroznia neutral vs unavailable
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- `enable_alpha_gate=false` daje neutralny mnoznik `1.0`.
- `enable_alpha_gate=true` i alpha actionable liczy quality z realnych `momentum`, `demand`, `joint`.
- `enable_alpha_gate=true` i alpha skipped/not-run/missing powoduje unavailable confidence, nie `0.0`.
- `V25ConfidenceBreakdown` zawiera `alpha_status` i `alpha_unavailable_reason`.

Zmiana 3: availability reason pokazuje alpha unavailability
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- `v25_confidence_availability()` zwraca alpha unavailable reason przed ogolnymi fallbackami missing-input.

Zmiana 4: Early/Normal shadow score nie nadpisuje canonical confidence
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- Lokalny `ShadowV25Decision.confidence` nadal moze sterowac stage behavior.
- `assessment.v25_confidence` jest wypelniane przez canonical `cache_v25_confidence()`, nie przez uproszczony stage score.
- Gdy canonical confidence jest unavailable, temporal snapshot emituje `v25_confidence = null` oraz `v25_confidence_available = false`.

Zmiana 5: buy-log JSONL dostal addytywne pola alpha status/reason
- Plik: `ghost-brain/src/oracle/decision_logger.rs`
- `GATEKEEPER_BUY_LOG_SCHEMA_VERSION` zwiekszono do `32`.
- Dodano opcjonalne pola:
  - `v25_confidence_alpha_status`,
  - `v25_confidence_alpha_unavailable_reason`.
- Istniejace `v25_confidence_alpha_quality` pozostaje, ale disabled-neutral jest teraz odroznialne przez status.
- Pola maja `#[serde(default, skip_serializing_if = "Option::is_none")]`.

Zmiana 6: backlog ML bez runtime integration
- Plik: `PLANS/BACKLOG_ALPHA_31100_CANDIDATE_V1_VALIDATION_HARNESS_20260624.md`
- `alpha_31100_candidate_v1` zostal opisany jako research/shadow-only kandydat do osobnego validation harness po PR4.
- Dokument zawiera zakazy: no live deploy, no BUY trigger, no F1 thresholding, no HTML rule copy, no Segment Lab as runtime rule source.

Zmiana 7: deterministyczny assessment clock dla timer/deadline parity
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- `run_assessment()` deleguje do `run_assessment_at(now_wall_ms)`.
- Timer/deadline fallback przekazuje ten sam wall-clock do assessment metadata (`observation_duration_ms`, `finalize_lag_ms`).
- `force_check_deadline()` nie nadpisuje juz `highest_seen_ts` wall-clockiem, gdy buffer ma event-time evidence z transakcji.
- Cel: zachowac parity canonical confidence dla Extended timer path i deadline fallback path bez zmiany progow decyzyjnych.

## 7. Walidacja dzialan naprawczych

Targeted validation wykonana na aktualnym checkoutcie:

```bash
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --lib v25_confidence -- --nocapture
# result: 7 passed

CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --lib force_check_deadline -- --nocapture
# result: 3 passed

CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_v25_regression confidence -- --nocapture
# result: 2 passed

CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_policy_tests alpha -- --nocapture
# result: 1 passed

CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_v25_regression -- --nocapture
# result: 41 passed

CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-brain test_serialization_to_json -- --nocapture
# result: 1 passed

CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-brain test_gatekeeper_buy_log_v19_without_v3_fields_deserializes -- --nocapture
# result: 1 passed
```

Wazna obserwacja z walidacji:
- Pierwszy przebieg `gatekeeper_v25_regression confidence` ujawnil rozjazd `p0_extended_timer_and_deadline_fallback_confidence_match`: timer confidence bylo `0.936589`, deadline fallback `0.0`.
- Przyczyna nie byla formula alpha, tylko nadpisanie event-time evidence przez deadline wall-clock w `force_check_deadline()`.
- Po naprawie `run_assessment_at(now_ms)` i zachowaniu event-time `highest_seen_ts` test przeszedl.

## 8. Co zostalo potwierdzone

Na poziomie kodu:
- Alpha disabled jest neutralnym komponentem wzoru, ale ma jawny status `neutral_disabled`.
- Alpha enabled/skipped/missing/not-run daje unavailable reason, nie numeric `0.0`.
- Canonical `v25_confidence` nie jest zastepowany uproszczonym Early/Normal shadow-stage score.
- Extended timer path i deadline fallback uzywaja deterministycznego assessment clock i nie traca event-time evidence.
- JSONL schema change jest addytywny.
- `alpha_31100_candidate_v1` nie jest podlaczony do BUY/REJECT.

Na poziomie testow:
- Testy PR3 potwierdzily disabled-neutral, insufficient-sample unavailable, missing-input unavailable, actionable measured quality oraz separacje Early/Normal stage score od canonical confidence.
- Regression subset potwierdzil parity confidence dla timer/deadline fallback.

## 9. Ryzyka resztkowe / czego PR3 jeszcze nie robi

- PR3 nie waliduje modelu ML `T0 + 31100 ms`; to osobny future validation harness po PR4.
- PR3 nie wybiera thresholda tradingowego.
- PR3 nie zmienia metryk alpha gate ani ich progow.
- PR3 nie usuwa starych shadow-stage score semantics, tylko chroni canonical confidence przed ich nadpisaniem.
- PR3 nie rozwiazuje niezaleznych baseline failures poza confidence path.
- PR3 nie czysci istniejacych warningow kompilacyjnych w repo; byly obecne poza zakresem tej zmiany.

## 10. Scope out

Poza zakresem PR3:
- deploy live,
- nowy BUY trigger,
- XGBoost/runtime alpha score,
- threshold tuning,
- F1 optimization,
- kopiowanie regul z HTML/Segment Lab,
- PR4 whale top3 semantic contract,
- master ledger / chronological OOS / ablation / leakage audit.

## 11. Kryteria zamkniecia

PR3 mozna uznac za domkniety, gdy:
- targeted tests dla `v25_confidence` przechodza,
- regression subset dla confidence przechodzi,
- gatekeeper policy alpha subset nie pokazuje regresji,
- DecisionLogger serialization/deserialization zachowuje kompatybilnosc,
- `git diff --check` przechodzi dla dotknietych plikow,
- finalny raport jawnie rozdziela PR3 confidence semantics od przyszlego `alpha_31100_candidate_v1` validation harness.
