# ADR-8D: HET Position Manager V2 PR A — prawda timeoutu i trwały writer health

Status: `IMPLEMENTED / LOCAL VALIDATION COMPLETE / PR #71 DRAFT`

Typ: ADR-8D / review remediation / aktywny shadow post-buy / evidence integrity / bounded async persistence

Data: 2026-07-17

Repo: `smahacfel/Gho`

Branch: `agent/het-pm-v2-pr-a`

Base SHA: `18d94b0cc5a226496a5ac2bc616e7488a7f78d5d`

Plan: `PLANS/DO_REALIZACJI/POSITION_MANAGER_HET_V2.md`, wyłącznie PR A, w szczególności §14–§15.

Powiązane ADR:

- `ADR_8D_HET_PM_V2_PR_A_OBSERVE_ONLY_20260716.md`;
- `ADR_8D_HET_PM_V2_PR_A_REVIEW_REMEDIATION_20260716.md`;
- `ADR_8D_HET_PM_V2_PR_A_BOUNDED_SIDECAR_TIMING_ISOLATION_20260716.md`.

Niniejszy dokument zastępuje poprzedni opis wszędzie, gdzie czwarty review
traktował każdy terminalny timeout acknowledgement jako jednoznaczne
`Skipped(writer_timed_out)`. Timeout po rozpoczęciu I/O ma teraz jawnie
nieznany wynik.

Poziom ryzyka: `MEDIUM-HIGH` — zmiana dotyka terminalnej korelacji,
asynchronicznego writera i kontraktu analizatora burn-in. Nie zmienia V1
policy, nie dodaje V2 authority, nie aktywuje live execution i nie tworzy
drugiego terminal/capacity ownera.

## 1. Problem

Piąty focused re-review PR #71 wykazał dwie pozostałe luki.

Po pierwsze, worker mógł rozpocząć `open/write/flush`, authority mogło
przekroczyć `terminal_write_budget_ms`, zapisać terminalne
`Skipped(writer_timed_out)`, a rozpoczęty syscall mógł później zakończyć się
sukcesem. Operational terminal truth deklarowałby wtedy brak wiersza, mimo że
ten sam `comparison_id` faktycznie pojawił się w sidecarze.

Po drugie, atomowe liczniki backpressure istniały wyłącznie w pamięci i były
widoczne tylko w testach. Analyzer znał liczbę zachowanych wierszy, ale nie
znał liczby prób enqueue, dropów kolejki, błędów I/O ani timeoutów. Selektywna
utrata obserwacji mogła więc wyglądać jak poprawna coverage.

## 2. Decyzja: atomowa maszyna stanu joba

Każdy job sidecara posiada współdzielony atomowy stan:

```text
Queued
  -> Writing
     -> Written | Failed
  -> CancelledBeforeWrite
```

Worker wykonuje CAS `Queued -> Writing` bezpośrednio przed pierwszym I/O.
Authority po utracie acknowledgement wykonuje CAS
`Queued -> CancelledBeforeWrite`. Dzięki temu dokładnie jedna strona ustala,
czy zapis jeszcze nie zaczął się, czy już wszedł do nieprzerywalnego I/O.

Terminalny wynik ma teraz trzy klasy:

```text
Written
Skipped { typed reason }
OutcomeUnknown { typed reason }
```

Mapowanie jest następujące:

- timeout w `Queued` -> `Skipped(writer_timed_out_before_write)`;
- timeout w `Writing` -> `OutcomeUnknown(writer_ack_timed_out)`;
- utrata kanału ack w `Writing` ->
  `OutcomeUnknown(writer_ack_channel_closed)`;
- zaobserwowane `Written` -> `Written`;
- zaobserwowane `Failed` -> `Skipped(writer_io_failed)`.

`OutcomeUnknown` nie blokuje canonical terminal commit ani capacity release.
Operational lifecycle record i canonical `TERMINAL_TRUTH.source_refs`
utrwalają osobno:

```text
comparison_write_status = outcome_unknown
comparison_outcome_unknown_reason = writer_ack_timed_out | writer_ack_channel_closed
comparison_id
source_snapshot_id
v1_action_id
```

Późny wiersz sidecara nie przeczy temu kontraktowi. Ostateczny wynik można
rozstrzygnąć przez reconciliation po `comparison_id` jako `written_late` albo
`absent_or_failed`.

## 3. Niezależny writer-health artifact

Primary comparison writer utrzymuje produkcyjne, atomowe cumulative counters:

```text
enqueue_attempts
enqueued
queue_full_drops
queue_closed_drops
writes_succeeded
writes_failed
cancelled_before_write
terminal_timeouts
terminal_outcome_unknown
```

Stan jest emitowany przez osobny OS thread i osobny bounded notification
channel. Health snapshot nie przechodzi przez kolejkę comparison, której
dropy mierzy. Powiadomienia mogą być koaleskowane, lecz snapshot zawsze czyta
najnowsze liczniki atomowe.

Każda instancja writera ma odrębny artefakt:

```text
het_pm_v2_writer_health_v1.<writer_instance_id>.json
```

Unikalna nazwa zapobiega utracie denominatora po restarcie writera, gdy ten sam
comparison JSONL jest kontynuowany. Snapshot jest zapisywany przez plik
tymczasowy i atomowy rename. Zawiera również:

```text
schema_version = 1
artifact_type = het_pm_v2_writer_health
writer_instance_id
run_id
mixed_run_ids
shutdown_complete
policy_id / policy_version / policy_config_hash
sidecar_path
started_at_ms / snapshot_generated_at_ms / revision
```

Brak artifactu, mixed run IDs, niezgodny config albo niepełny shutdown nie są
interpretowane jako zero dropów. Oznaczają nieznaną coverage.

## 4. Bounded shutdown finalization

PostBuyRuntime po zatrzymaniu primary monitor loop wywołuje jawne:

```text
flush_het_pm_v2_writer_health_for_shutdown()
```

Metoda:

1. czeka maksymalnie 250 ms na quiescent counters;
2. ustawia `shutdown_complete=true` tylko po rozliczeniu wszystkich enqueue;
3. wykonuje finalny snapshot przez `spawn_blocking`;
4. czeka na niego maksymalnie 250 ms.

To jest ścieżka shutdown, nie authority tick ani terminal capacity path.
Przekroczenie budżetu pozostawia coverage jako unknown i nie zatrzymuje
zamknięcia runtime. Jeżeli po finalizacji pojawiłby się nowy enqueue,
`shutdown_complete` jest natychmiast cofane do `false`.

## 5. Kontrakt analizatora

`scripts/het_pm_v2_analysis.py` wymaga teraz co najmniej jednego:

```text
--writer-health het_pm_v2_writer_health_v1.<id>.json
```

Przy wielu instancjach parametr jest powtarzany. Manifest raportu utrwala hash
i pełną identity każdego health inputu. Analyzer waliduje schema, typy,
identity, monotoniczne ograniczenia liczników, clean shutdown oraz zgodność:

```text
enqueued + queue_full_drops + queue_closed_drops == enqueue_attempts
writes_succeeded + writes_failed + cancelled_before_write <= enqueued
writes_succeeded == liczba faktycznych comparison rows
run_id i HET config hash health == comparison input
```

Raport zawiera:

```text
capture_ratio = writes_succeeded / enqueue_attempts
drop_ratio
io_failure_ratio
terminal_timeout_count
terminal_outcome_unknown_count
pending_or_inflight_count
writer_health_evidence_status
coverage_unknown
promotion_evidence_available
```

Statusy mają jawne znaczenie:

- `complete` — clean shutdown, spójne liczniki, zero observation loss;
- `complete_with_observation_loss` — denominator jest znany, ale wystąpił
  drop, błąd, anulowanie albo outcome unknown;
- `incomplete_or_inconsistent` — coverage nie może zostać ustalona;
- `missing` — brak wymaganego artifactu.

Tylko `complete` ustawia lokalne
`writer_health.promotion_evidence_available=true`. Nie oznacza to zaliczenia
Gate 4/5 ani promocji PR B; oznacza wyłącznie, że producent nie ukrywa
brakującego denominatora.

## 6. Testy regresyjne

Dodano lub rozszerzono testy:

```text
terminal_timeout_after_writer_started_is_not_reported_as_skipped
writer_health_artifact_durably_exposes_nonterminal_queue_drops
terminal_het_writer_timeout_marks_skipped_and_continues_canonical_commit
terminal_writer_timeout_preserves_comparison_id_and_typed_skip_reason
test_missing_writer_health_marks_coverage_unknown
test_writer_health_exposes_dropped_denominator
test_unclean_writer_health_cannot_support_promotion
test_invalid_writer_health_counters_are_rejected
```

Kontrolowany writer sygnalizuje wejście do I/O, blokuje zakończenie aż do
przekroczenia budżetu, a następnie pozwala zapisać wiersz. Test dowodzi
jednocześnie `OutcomeUnknown`, nieprzerwanego canonical terminal/capacity flow
i późnej korelacji dokładnie tego samego `comparison_id`.

## 7. Inwarianty zachowane

- V1 pozostaje jedynym proposal/apply/terminal/capacity ownerem;
- V2 nie konsumuje własnego wyniku i nie tworzy live authority;
- terminal comparison nadal pochodzi z oryginalnego pre-mutation snapshotu;
- terminal retry nie wykonuje ponownej ewaluacji V2;
- comparison i health queues są bounded;
- filesystem I/O nie wraca do Tokio authority tasku;
- terminal oczekuje na comparison writer wyłącznie przez istniejący,
  skonfigurowany budżet;
- health shutdown flush jest osobny, ograniczony czasowo i nie uczestniczy w
  canonical terminal receipt;
- health artifact nie jest terminal SSOT ani policy inputem;
- brak health evidence degraduje promotion coverage do unknown;
- nie uruchomiono shadow burn-inu ani Gate 4/5.

## 8. Zakres plików

- `ghost-brain/src/guardian/post_buy/engine.rs` — atomic job state,
  `OutcomeUnknown`, niezależny health reporter, bounded shutdown flush i fault
  injection;
- `ghost-brain/src/guardian/post_buy/exit_policy_v2.rs` — typed timeout/unknown
  reasons i correlation `run_id`;
- `ghost-launcher/src/components/post_buy_runtime.rs` — finalny bounded health
  flush na shutdownie primary monitora;
- `scripts/het_pm_v2_analysis.py` — wymagany health input i denominator
  coverage contract;
- `scripts/test_het_pm_v2_analysis.py` — testy health schema/coverage;
- niniejszy ADR-8D.

## 9. Lokalna walidacja

| Kontrola | Wynik |
| --- | --- |
| `cargo check -p ghost-launcher --lib` | PASS. |
| `cargo test -p ghost-brain guardian::post_buy --lib` | PASS — 240/240. |
| exact timeout-after-writing fault injection | PASS — 1/1. |
| exact durable queue-drop health artifact test | PASS — 1/1. |
| istniejące terminal timeout-before-write tests | PASS. |
| exact PostBuyRuntime shutdown test | PASS — 1/1. |
| `python3 -m unittest scripts/test_het_pm_v2_analysis.py` | PASS — 18/18. |
| `python3 -m py_compile scripts/het_pm_v2_analysis.py scripts/test_het_pm_v2_analysis.py` | PASS. |
| exact diff-scoped Clippy względem base SHA | PASS — brak nowych diagnostics i primary spans w zmienionym Rust. |
| `cargo fmt --all -- --check` | PASS. |
| `git diff --check` | PASS. |

Zgodnie z zakresem PR A nie uruchomiono shadow burn-inu i nie policzono CVaR,
MFE capture, tail losses ani creator/funder cohorts.

## 10. Rollback

Rollback to revert jednego commita remediation do headu `1ee50bbe...`.
Przywróciłby on jednak dwie luki: fałszywie kategoryczny timeout status oraz
brak trwałego denominatora dropów. Taki stan nie powinien być używany jako
producer promotion evidence.

Bezpieczny operacyjny kill switch pozostaje bez zmian:

```toml
[post_buy_guardian.het_pm_v2]
enabled = false
```

Wyłączenie HET nie zmienia V1 lifecycle ani terminal/capacity ownership.
