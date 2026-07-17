# ADR-8D: HET Position Manager V2 PR A — producer denominator i per-writer health join

Status: `IMPLEMENTED / LOCAL VALIDATION PASSED / PR #71 DRAFT`

Typ: ADR-8D / review remediation / aktywny shadow post-buy / evidence integrity / analyzer contract

Data: 2026-07-17

Repo: `smahacfel/Gho`

Branch: `agent/het-pm-v2-pr-a`

Base SHA: `18d94b0cc5a226496a5ac2bc616e7488a7f78d5d`

Plan: `PLANS/DO_REALIZACJI/POSITION_MANAGER_HET_V2.md`, wyłącznie PR A.

Powiązane ADR:

- `ADR_8D_HET_PM_V2_PR_A_OBSERVE_ONLY_20260716.md`;
- `ADR_8D_HET_PM_V2_PR_A_REVIEW_REMEDIATION_20260716.md`;
- `ADR_8D_HET_PM_V2_PR_A_BOUNDED_SIDECAR_TIMING_ISOLATION_20260716.md`;
- `ADR_8D_HET_PM_V2_PR_A_TIMEOUT_TRUTH_AND_WRITER_HEALTH_20260717.md`.

Niniejszy ADR rozszerza poprzedni kontrakt writer-health. Poprzednia wersja
mierzyła denominator od momentu `try_enqueue()`. Ta korekta przesuwa granicę
denominatora przed lokalną walidację comparison i wiąże każdy comparison row z
konkretną instancją writera.

Poziom ryzyka: `MEDIUM` — zmiana dotyka trwałego kontraktu evidence i
analizatora burn-in. Nie zmienia V1 policy, nie dodaje V2 authority, nie
aktywuje live execution i nie wprowadza nowego terminal/capacity ownera.

## 1. Problem

Szósty focused re-review PR #71 wykazał dwie luki kontraktu evidence.

Pierwsza luka: health counter `enqueue_attempts` zaczynał się dopiero w
`writer.try_enqueue()`. Jeżeli comparison został odrzucony wcześniej przez
lokalną walidację lub serializację, obserwacja nie trafiała ani do sidecara,
ani do denominatora writer-health.

Przykład fałszywego sukcesu:

```text
100 kwalifikujących się ticków HET
10 lokalnych validation/serialization skips
90 gotowych do enqueue
90 zapisanych wierszy

stary health:
  enqueue_attempts = 90
  writes_succeeded = 90
  capture_ratio = 100%
```

W rzeczywistości end-to-end capture wynosił 90%, a 10% obserwacji zniknęło
przed granicą enqueue. Takie skippy są same w sobie ważnym evidence, ponieważ
mogą wskazywać naruszenie snapshot contractu, błąd semantic validation,
przekroczenie payload budget albo selektywną utratę terminalnych ticków.

Druga luka: analyzer sumował liczniki health globalnie. Przy analizie wielu
runów lub wielu writer instances brakujące wiersze w jednej grupie mogły być
skompensowane nadmiarem w innej grupie. Health artifact zawierał
`writer_instance_id` i `sidecar_path`, ale comparison rows nie posiadały
`writer_instance_id`, więc analyzer nie mógł wymusić jednoznacznego joinu.

## 2. Decyzja: producer-level denominator przed walidacją

Writer-health otrzymuje dwa poziomy denominatora:

```text
comparison_attempts
comparison_ready_for_enqueue

core_validation_skips
final_validation_skips
serialization_skips
payload_oversized_skips
```

Inwariant producenta:

```text
comparison_ready_for_enqueue
+ core_validation_skips
+ final_validation_skips
+ serialization_skips
+ payload_oversized_skips
= comparison_attempts
```

`comparison_attempts` jest inkrementowane bezpośrednio po przygotowaniu
`PreparedV1V2ComparisonCoreV1`, czyli przed finalną walidacją, serializacją,
size checkiem i enqueue.

`comparison_ready_for_enqueue` jest inkrementowane tylko dla finalnego
`PreparedHetComparisonV1::Ready`.

`enqueue_attempts` pozostaje licznikiem niższej warstwy I/O i musi być równe
`comparison_ready_for_enqueue`.

Analyzer raportuje trzy osobne miary:

```text
producer_validity_ratio = comparison_ready_for_enqueue / comparison_attempts
writer_capture_ratio    = writes_succeeded / enqueue_attempts
end_to_end_capture_ratio = writes_succeeded / comparison_attempts
```

Pole `capture_ratio` w raporcie oznacza teraz end-to-end capture ratio, a nie
wyłącznie writer queue capture.

Każdy lokalny skip oznacza znaną utratę obserwacji:

```text
writer_health_evidence_status = complete_with_observation_loss
promotion_evidence_available = false
```

Brak lub niespójność producer denominatora oznacza:

```text
writer_health_evidence_status = incomplete_or_inconsistent
coverage_unknown = true
promotion_evidence_available = false
```

## 3. Decyzja: writer_instance_id w każdym comparison row

Każdy `V1V2ComparisonRecord` zapisuje obowiązkowe:

```text
writer_instance_id
```

Wartość pochodzi ze stabilnego `HetPmV2ObservationWriterV1` aktywnego dla
danego monitora. Ta sama wartość jest przenoszona do:

```text
HetComparisonCorrelationV1
ShadowLifecycleRecord.het_pm_v2_writer_instance_id
TERMINAL_TRUTH.source_refs
```

Analyzer grupuje comparison rows po:

```text
writer_instance_id + run_id + policy_config_hash
```

Dla każdego health artifactu wymagany jest dokładnie jeden odpowiadający
bucket rows:

```text
health.writer_instance_id == rows.writer_instance_id
health.run_id == rows.run_id
health.policy_config_hash == rows.policy_config_hash
health.writes_succeeded == liczba rows w tej grupie
```

Każdy comparison row musi mieć dokładnie jeden health artifact. Każdy health
artifact musi mapować się do dokładnie jednej grupy comparison rows. Analyzer
dodatkowo porównuje `health.sidecar_path` z plikiem inputu, z którego
pochodziły rows. Przy istniejących ścieżkach używana jest postać
zresolverowana, a przy nieistniejących surowy tekst ścieżki.

Ten kontrakt eliminuje kompensację braków pomiędzy:

- runami;
- restartami writera;
- różnymi plikami sidecara;
- przypadkowo zgodnymi globalnymi sumami.

## 4. Schema evolution

Zmieniono wersje trwałych kontraktów:

```text
HET_PM_V2_SCHEMA_VERSION = 2
HET_PM_V2_WRITER_HEALTH_SCHEMA_VERSION = 2
```

Analyzer odrzuca mieszanie schema version. Starsze rekordy bez
`writer_instance_id` nie spełniają kontraktu PR A po tej korekcie i nie mogą
być użyte jako promotion evidence.

## 5. Konsekwencje

Po tej zmianie burn-in evidence ma jawny denominator obejmujący cały producent:

```text
prepare comparison core
-> local validation / serialization / size check
-> enqueue
-> queue/write/timeout outcome
```

Analyzer może odróżnić:

- brak obserwacji przez lokalny błąd producer contractu;
- brak obserwacji przez pełną lub zamkniętą kolejkę;
- brak obserwacji przez błąd I/O;
- nieznany wynik terminalnego timeoutu po rozpoczęciu I/O;
- pełne i spójne zapisanie wszystkich obserwacji.

Nie oznacza to zaliczenia późniejszych Gate 4/5. To jest wyłącznie kontrakt
wiarygodności producenta evidence dla PR A. Metryki takie jak CVaR, MFE
capture ratio, tail losses i segmentacja kohortowa nadal wymagają osobnego
burn-in datasetu oraz osobnej analizy na danych runtime.

## 6. Testy regresyjne

Dodano lub rozszerzono testy:

```text
core_validation_skip_is_included_in_producer_denominator
final_validation_skip_is_included_in_producer_denominator
serialization_or_oversized_skip_prevents_complete_health_status
end_to_end_capture_ratio_uses_all_comparison_attempts
writer_health_cannot_compensate_missing_rows_between_runs
writer_health_cannot_compensate_missing_rows_between_instances
health_artifact_for_another_sidecar_is_rejected
every_comparison_row_requires_exact_writer_instance_match
writer_health_artifact_durably_exposes_nonterminal_queue_drops
```

Wymagane walidacje lokalne dla tej korekty:

```text
cargo fmt --check
python3 -m py_compile scripts/het_pm_v2_analysis.py scripts/test_het_pm_v2_analysis.py
python3 -m unittest scripts/test_het_pm_v2_analysis.py
cargo check -p ghost-launcher --lib
cargo test -p ghost-brain terminal_timeout_after_writer_started_is_not_reported_as_skipped --lib
cargo test -p ghost-brain writer_health_artifact_durably_exposes_nonterminal_queue_drops --lib
cargo test -p ghost-brain guardian::post_buy --lib
python3 scripts/guard_diff_scoped_clippy.py --base 18d94b0cc5a226496a5ac2bc616e7488a7f78d5d --head HEAD
```

## 7. Inwarianty zachowane

- V2 nadal nie tworzy proposal.
- V2 nadal nie wykonuje apply.
- V2 nadal nie zamyka pozycji.
- V2 nadal nie zwalnia capacity.
- Terminal canonical commit pozostaje authority V1.
- Sidecar pozostaje observe-only.
- Writer I/O pozostaje poza głównym authority taskiem.
- Terminal wait pozostaje bounded.
- Health artifact nie jest zapisywany przez mierzoną kolejkę comparison.
- Analyzer nie promuje coverage przy brakującym, niespójnym albo niepełnym
  writer-health.
