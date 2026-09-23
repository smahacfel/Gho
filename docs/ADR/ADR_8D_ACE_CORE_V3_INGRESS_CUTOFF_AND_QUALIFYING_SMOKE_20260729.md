# ADR-8D: ACE Core V3 — ingress cutoff i kwalifikujący smoke przed Dniem 1

Status: `IMPLEMENTED / FOCUSED_VALIDATION_PASS / QUALIFYING_SMOKE_PENDING /
DAY1_NOT_STARTED / OBSERVE_ONLY / PR2_STILL_BLOCKED`

Typ: ADR-8D / remediation decision-time integrity / capture-health evidence

Data: `2026-07-29`

Repo: `smahacfel/Gho`

Baseline naukowy: `origin/main = 43057b296663129ca9b4f572e793474830a5452c`

Plan SSOT:
`PLANS/DO_REALIZACJI/PLAN_ACE_CORE_ONE_DAY_KILL_TEST_V3_POST_PR86.md`

Poprzedni ADR integrity:
`docs/ADR/ADR_8D_ACE_CORE_V3_CAPTURE_INTEGRITY_GO_GATE_20260729.md`

Uwaga o szablonie: ścieżka wskazana w instrukcji globalnej,
`/Gho/docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Dokument
zachowuje lokalny format ADR-8D stosowany w `docs/ADR/`.

## D0. Decyzja

Przed kwalifikującym smoke i 24-godzinnym Dniem 1 ACE Core V3 musi dowodzić
dwóch rzeczy, których poprzednia bramka nie dowodziła wystarczająco:

1. feature i entry state były nie tylko zapisane przed chain cutoffem, ale
   także faktycznie dostarczone do runtime przed granicą decyzji;
2. health receipt pochodzi z właściwego runu, obejmuje właściwy czas i nie
   może pozostać pozornie zdrowym artefaktem po nieudanym `finalize`.

Korekta dodaje te bramki wyłącznie do offline probe i narzędzia operatorowego.
Nie uruchamia capture, nie tworzy actor/lifecycle/replay subsystemu i nie
zmienia Gatekeepera, `MaterializedFeatureSet`, PR1E authority, quote math,
Position Managera, Triggera ani PR2.

## D1. Dwie osie cutoffu bez lookahead

Każdy birth ma oddzielne czasowe granice:

```text
decision_event_cutoff_ms   = birth_ts_ms + 11_111
birth_ingress_ms           = detected_wall_ts_ms
decision_ingress_cutoff_ms = birth_ingress_ms + 11_111
```

Feature BUY i entry reserve state są legalne tylko gdy równocześnie:

```text
event_ts_ms   <= decision_event_cutoff_ms
arrival_ts_ms <= decision_ingress_cutoff_ms
```

`detected_wall_ts_ms` birth oraz `arrival_ts_ms` successful BUY muszą być
dodatnie. Ich brak daje typed `NON_EVALUABLE_FEATURES`; nie istnieje fallback
do chain timestampu, `sol_amount_lamports`, latest state ani innego pola.

Entry reserve state zachowuje wcześniejsze wymagania `success = true`,
`is_synthetic = Some(false)`, `complete = Some(false)`, complete reserves i
świeżość event time. Post-cutoff observed outcome pozostaje outcome-only;
nie jest to input decyzji i nie otrzymuje sztucznej bramki ingress.

Pierwsze 250 feature-evaluable rows do kalibracji jest porządkowane przede
wszystkim przez `detected_wall_ts_ms`, następnie przez istniejący deterministic
canonical tie-break. Dzięki temu skala i threshold odzwierciedlają kolejność,
w której runtime mógł obserwować kandydatów, a nie tylko kolejność czasu
łańcuchowego.

## D2. Self-invalidating health receipt v2

`scripts/ace_core_one_day_capture_health.py snapshot` zapisuje immutable JSON
z:

```text
run_id
manifest_sha256
phase = start | end
capture_kind = smoke | day1
captured_at_unix_ms
required PR1 counters
raw_metrics_sha256
```

`finalize` fail-closed wymaga zgodności obu snapshotów z manifestem i ze sobą,
ich kolejności czasu oraz czasowego kontraktu:

```text
smoke: 120_000 ms <= end - start <= 300_000 ms
day1:  end - start >= 86_400_000 ms
```

Przy dowolnym failure `finalize` zwraca kod `2` **bez utworzenia** pliku pod
`health_evidence_path`. Offline probe widzi wtedy brak wymaganej evidence i
oznacza run `INVALID_CAPTURE`; nie ma artefaktu z fikcyjnymi zerami counterów.

Receipt v2 przechowuje SHA obu snapshotów, SHA surowych body metryk, start/end
time i duration. Probe waliduje te pola wraz z run ID, SHA manifestu,
counterami i flagami controlled shutdown/clean flush/log cleanliness.

Forbidden log gate obejmuje dodatkowo:

```text
RUG_SCALP_RUNTIME_FEE_AUTHORITY_INVALIDATED
RUG_SCALP_RUNTIME_FEE_AUTHORITY_CHANGE_REQUESTED_CONTROLLED_SHUTDOWN
Oracle Runtime failed before shutdown signal
Oracle Runtime task failed before shutdown signal
Oracle Runtime component shutdown failed
Component shutdown completed with <n> failure(s) or forced abort(s)
```

## D3. Symetryczne pokrycie kohort

Summary zachowuje dotychczasową ogólną `evaluable_coverage_pct` jako
diagnostykę, ale raportuje i stosuje bramkę osobno:

```text
selected_feature_count
rest_feature_count
selected_outcome_evaluable_count
rest_outcome_evaluable_count
selected_outcome_coverage_pct
rest_outcome_coverage_pct
```

`ACE_PROBE_PROMISING_NOT_PROVEN` wymaga teraz co najmniej 50% outcome coverage
w **obu** kohortach. Zapobiega to sytuacji, w której tylko najsłabsza część
SELECTED znika na capacity/reserves/sustain coverage, a średnia jest liczona
z niereprezentatywnej reszty.

## D4. Kwalifikujący smoke

Smoke trwa 2–5 minut w świeżym run ID i nowych ścieżkach. Po poprawnym
`finalize` musi uruchomić dokładnie binarkę
`ace_core_one_day_probe` na własnych artifacts. `verify-probe` akceptuje
wyłącznie summary z:

```text
capture_status = VALID_CAPTURE
capture_invalid_reasons = []
```

Mała liczba rows może zakończyć sam eksperyment jako
`ACE_PROBE_INCONCLUSIVE`; nie dyskwalifikuje smoke'a, jeśli integrity status
jest prawidłowy. Artefakty smoke nigdy nie są Dniem 1.

`GO` dla Dnia 1 wymaga łącznie:

1. start/end snapshotów związanych z manifestem tego samego runu;
2. czasu smoke 120–300 s i `finalize` z kodem 0;
3. receipt v2 pod oczekiwaną ścieżką;
4. offline probe z `VALID_CAPTURE` i bez invalid reasons;
5. focused tests oraz release build na zamrożonym source/config SHA.

## D5. Weryfikacja

Focused Rust suite obejmuje pre-cutoff BUY dostarczony po ingress cutoffie,
spóźniony reserve entry state, brak birth ingress timestampu, kolejność
kalibracji według ingress, per-cohort coverage i contract health receipt v2.

Python unittest helpera obejmuje prawidłowy manifest-bound smoke, brak
wymaganego countera bez pozostawienia receipt, fee-authority invalidation marker
i akceptację wyłącznie `VALID_CAPTURE` z pustymi invalid reasons.

Wymagane przed operacyjnym `GO`:

```text
cargo fmt --all --check
cargo test -p ghost-launcher ace_core_one_day_probe --lib -- --nocapture
cargo test -p ghost-launcher oracle_metrics --lib -- --nocapture
cargo test -p ghost-launcher metrics_server_tests --bin ghost-launcher -- --nocapture
python3 scripts/test_ace_core_one_day_capture_health.py
cargo build --release -p ghost-launcher --bin ghost-launcher --bin ace_core_one_day_probe
```

## D6. Granice i rollback

Zmiana pozostaje diagnostic-only i observe-only. Nie powoduje BUY, shadow
submit, live execution, nowej polityki, zmiany sizingu, modelu ML ani
automatycznej promocji PR2. Rollback jest prosty: nie uruchamiać nowego
rolloutu; nie istnieje aktywna ścieżka rynkowa do cofnięcia.

Ten ADR nie jest zgodą na 24 h capture. Do jego uruchomienia potrzebny jest
osobny, udokumentowany `PASS` kwalifikującego smoke'a.
