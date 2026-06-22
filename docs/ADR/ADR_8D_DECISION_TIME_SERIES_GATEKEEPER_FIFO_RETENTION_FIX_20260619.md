# ADR-8D: Decision Time Series retention beyond Gatekeeper dedupe FIFO

Status: IMPLEMENTED / TARGETED_TEST_VERIFIED / RUNTIME_RESTART_REQUIRED
Typ: ADR-8D / runtime evidence retention repair
Data: 2026-06-19
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR update time
Zakres: naprawa residualu, w ktorym `decision_time_series` zatrzymywal sie praktycznie na 256 probkach mimo profilu `decision_time_series_tx_capacity = 4096`
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/session/observation.rs`
- `ghost-launcher/tests/tx_intelligence_tests.rs`
- `docs/ADR/ADR_8D_DECISION_TIME_SERIES_GATEKEEPER_FIFO_RETENTION_FIX_20260619.md`

Powiazane ADR:
- `docs/ADR/ADR_8D_DECISION_TIME_SERIES_RETENTION_AND_PR6_REAL_EXPORT_PROOF_20260619.md`
- `docs/ADR/ADR_8D_DTW_DECISION_SERIES_AND_TEMPORAL_DELTAS_20260618.md`
- `docs/ADR/ADR_8D_RUNTIME_GAPS_TOP_LEVEL_EVIDENCE_CPV_DTW_20260619.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w istniejacych ADR-ach PR1-PR6 i runtime repair ADR-ach.

## 1. Przygotowanie i dzialania wstepne

Problem:
Runtime R38 po rebuildzie potwierdzil, ze `decision_time_series_tx_capacity` w payloadzie i `evidence_policy_context` wynosi `4096`, ale czesc rekordow nadal miala:
- `decision_time_series.retained_sample_count = 256`,
- `decision_time_series.total_tx_count > 256`,
- `decision_time_series.dropped_oldest_count > 0`,
- `decision_time_series.retention_status = "truncated"`.

To oznaczalo, ze limit 128 zostal usuniety, ale pojawil sie drugi praktyczny cap na 256.

Akcje wstepne:
- Przeliczono aktualne artefakty R38 z hasha configu `a6e60a8f9318af9a2ffdd024c285638af3adc2e9f9cf4608254519dfc5cb27ce`.
- Potwierdzono, ze `gatekeeper_v2_config_payload.decision_time_series_tx_capacity = 4096`.
- Potwierdzono, ze `evidence_policy_context.decision_time_series_tx_capacity = 4096`.
- Potwierdzono, ze `decision_time_series.retention_capacity = 4096`.
- Zidentyfikowano, ze truncated rekordy zatrzymuja sie dokladnie na 256 retained samples.

## 2. Routing i skills

Uzyte skills:
- `ghost-execution`: ochrona MaterializedFeatureSet, DecisionLogger/audit path i brak zmiany Gatekeeper policy.
- `rust-master`: zmiana dotyka Rust hot-path `PoolObservationSession::ingest_transaction`.
- `large-data-analytics`: runtime artifact audit i porownanie record-level evidence.

Nie ladowano dokumentow specjalistycznych:
- `solana-execution-path-engineer`: brak zmian TX buildera, sendera, blockhash, retry lub confirmation.
- `seer-ingest-event-integrity-specialist`: brak zmian parserow, Yellowstone/Geyser streamow lub event identity.
- `config-rollout-safety-reviewer`: config 4096 byl juz poprawnie zaladowany; problem nie byl w configu.

## 3. Opis problemu - 3W2H

What:
`PoolObservationSession` dopisywal transakcje do `decision_time_series` tylko wtedy, gdy liczba unikalnych kluczy w `GatekeeperBuffer` wzrosla. Ten set ma rotujacy FIFO cap `tx_keys_capacity = min_tx_count * 8 max 256`. Po osiagnieciu 256 nowa zaakceptowana transakcja mogla zastapic stary klucz, ale rozmiar setu pozostawal 256. Sesja uznawala wtedy, ze transakcja nie jest accepted-unique i nie dopisywala jej do `decision_time_series`.

Where:
- `PoolObservationSession::ingest_transaction`.
- Test-only mirror `PoolObservationSession::legacy_test_verdict_from_transaction`.
- `GatekeeperBuffer` exposed only `unique_tx_key_count()`, ktory nie jest monotoniczny.

Why it matters:
Dla DTW, selector evidence i replay/audit pełna seria tickow musi odzwierciedlac zaakceptowane ticki w observation window. Config 4096 nie moze byc omijany przez drugi, ukryty limit 256.

How observed:
W aktualnym R38:
- v25 shadow: 20 rekordow `truncated`, max retained `256`, max `total_tx_count` ponad 400.
- legacy live: analogiczne truncated rekordy.
- `retention_capacity` w rekordach wynosil `4096`, wiec problem nie byl w profilu.

How many / scale:
Problem ujawnia sie dla bardzo aktywnych tokenow z ponad 256 accepted tx w oknie obserwacji. Mniejsze rekordy byly poprawne.

## 4. Przyczyna zrodlowa

Root cause:
Kod uzywal bounded set length jako proxy dla "czy GatekeeperBuffer zaakceptowal nowa unikalna transakcje":

```rust
let prior_unique = self.gatekeeper_buffer.unique_tx_key_count();
let outcome = self.gatekeeper_buffer.ingest_transaction_tracking_only(tx.clone());
let accepted_unique = self.gatekeeper_buffer.unique_tx_key_count() > prior_unique;
```

To dziala tylko dopoki set rosnie. Po osiagnieciu FIFO cap 256 set moze pozostawac tej samej dlugosci, mimo ze `GatekeeperBuffer` rzeczywiscie przyjal nowy tx i zwiekszyl monotoniczny `total_tx_count`.

Wniosek:
`unique_tx_key_count()` jest bounded-size diagnostic, nie acceptance counter. Nie wolno go uzywac jako sygnalu accepted event w retention path.

## 5. Strategia naprawy

Przyjeta strategia:
- Nie zwiekszac arbitralnie `tx_keys_capacity`.
- Nie zmieniac Gatekeeper policy, verdictow ani dedupe semantics.
- Nie retainowac dust/duplicate/unknown tx po cichu.
- Wykrywanie accepted tx oprzec o monotoniczny `GatekeeperBuffer.total_tx_count`.
- Dodac test regresji, ktory przekracza FIFO 256 przy decision-series capacity 512.

Granice:
- Brak zmian DecisionLogger schema.
- Brak zmian MaterializedFeatureSet shape.
- Brak zmian CPV/FSC/Jito/flipper logic.
- Brak zmian shadow/live execution.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: monotoniczny accessor
Dodano:

```rust
pub const fn total_tx_count(&self) -> usize
```

w `GatekeeperBuffer`.

Zmiana 2: session accepted-tx detection
W `PoolObservationSession::ingest_transaction` oraz test-only mirrorze `legacy_test_verdict_from_transaction` zamieniono:
- bounded `unique_tx_key_count()` delta

na:
- monotoniczny `total_tx_count()` delta.

Efekt:
`decision_time_series` retainuje kazda transakcje, ktora GatekeeperBuffer faktycznie zaakceptowal jako normalny tx, takze po przekroczeniu FIFO 256.

Zmiana 3: test regresji
Dodano test:

```text
session_decision_time_series_retains_beyond_gatekeeper_dedupe_fifo_capacity
```

Test ustawia:
- `decision_time_series_tx_capacity = 512`,
- domyslne `min_tx_count`, wiec `GatekeeperBuffer.tx_keys_capacity = 256`,
- 300 unikalnych tx.

Acceptance:
- `tx_buffer.len() == 300`,
- `decision_time_series.sample_count == 300`,
- `decision_time_series.total_tx_count == 300`,
- `dropped_oldest_count == 0`,
- `retention_status == Clean`.

## 7. Walidacja

Wykonane komendy:
- `cargo fmt --package ghost-launcher`
- `cargo test -q -p ghost-launcher session_decision_time_series --test tx_intelligence_tests`
- `git diff --check -- ghost-launcher/src/components/gatekeeper.rs ghost-launcher/src/session/observation.rs ghost-launcher/tests/tx_intelligence_tests.rs scripts/analiza_porownawcza.py analiza_porownawcza.py docs/ADR/ADR_8D_ANALIZA_POROWNAWCZA_DTW_SECTION6_BUDGET_GUARD_20260619.md`

Wynik:
- Formatowanie zakonczone bez bledu.
- Targetowane testy: `2 passed`.
- `git diff --check` czysty.

Uwaga:
Test command emituje istniejace warningi workspace, niezwiązane z ta zmiana.

## 8. Ryzyka i zabezpieczenia

Ryzyko 1: retained decision series zacznie zawierac tx, ktore Gatekeeper nie akceptuje.
Mitigacja:
- Sygnal acceptance pochodzi z `GatekeeperBuffer.total_tx_count`, ktory rosnie w `update_tracking` tylko na accepted non-dust, non-duplicate path.
- Nie retainujemy po samym przyjsciu eventu.

Ryzyko 2: wieksza liczba retained tx zwiekszy pamiec na aktywna sesje.
Mitigacja:
- Nadal obowiazuje `decision_time_series_tx_capacity`.
- Base default pozostaje konserwatywny.
- Profile R37/R38 maja jawny wyzszy capacity 4096.

Ryzyko 3: zmiana Gatekeeper verdict behavior.
Mitigacja:
- Nie zmieniono `ingest_transaction_tracking_only` return policy.
- Nie zmieniono `evaluate_phases`.
- Nie zmieniono thresholds ani reason codes.

Ryzyko 4: aktywny runtime run nie potwierdzi fixu.
Mitigacja:
- Obecny tmux run dziala na starej binarce.
- Runtime proof wymaga rebuild/restart i nowych rekordow po tej zmianie.

## 9. Status koncowy

Status: implemented, requires fresh runtime proof after rebuild/restart.

Kodowa przyczyna residualu 256 zostala naprawiona. Aktualny aktywny run R38 nie moze potwierdzic tej poprawki, bo zostal uruchomiony przed patchem. Po kolejnym rebuild/restart acceptance dla decision series powinno sprawdzac:
- `decision_time_series.retention_capacity == 4096`,
- rekordy z `total_tx_count > 256` maja `retained_sample_count == total_tx_count`, o ile `total_tx_count <= 4096`,
- `dropped_oldest_count == 0` dla rekordow ponizej capacity,
- top-level `vectors_prices`, `vectors_ts_offsets_ms`, `vectors_sol_amounts` zachowuja dlugosc embedded serii.
