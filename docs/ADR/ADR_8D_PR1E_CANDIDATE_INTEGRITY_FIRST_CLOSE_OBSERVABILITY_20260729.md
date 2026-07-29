# ADR-8D — PR1E: obserwowalność pierwszego zamknięcia CandidateIntegrity

**Data:** 2026-07-29
**Status:** IMPLEMENTED / DIAGNOSTIC-SMOKE-PENDING / DAY1-BLOCKED
**Zakres:** `ghost-launcher/src/candidate_integrity.rs` oraz diagnostyka błędu
receipt staging w `ghost-launcher/src/components/seer.rs`

## D0. Decyzja

Dodano wyłącznie fail-closed obserwowalność pierwszego przejścia
`candidate_admission_open: true -> false`.

Normalne zamknięcie emituje dokładnie raz:

```text
CANDIDATE_INTEGRITY_ADMISSION_CLOSED
```

z polami `reason`, `close_path`, `authority_epoch_id`,
`admission_generation` oraz `registry_available`. `close_path` ma wartość
`state_locked` albo `mutex_poison_fallback`.

Tylko rzeczywisty błąd `Mutex::lock() -> PoisonError` emituje dodatkowo:

```text
CANDIDATE_INTEGRITY_STATE_MUTEX_POISONED
```

z `authority_epoch_id`, `admission_generation`,
`candidate_admission_open` i `registry_available`. Nie używa się
`PoisonError::into_inner()`, nie resetuje registry i nie otwiera ponownie
admission.

`RegistryUnavailable` ma teraz neutralny tekst:

```text
candidate integrity registry is unavailable
```

Nie jest już fałszywie przedstawiany jako dowód poisoningu, ponieważ ten sam
wariant błędu jest zwracany także dla zamkniętego admission i
`available=false`.

## D1. Powód

Unieważniony Day 1 pokazał najpierw:

```text
Seer: canonical apply receipt staging failed
error=candidate integrity registry mutex is poisoned
```

ale ówczesny tekst `RegistryUnavailable` nie rozróżniał prawdziwego
`PoisonError` od wcześniejszego zamknięcia admission lub niedostępnego
registry. Następny `receipt_stage_failed` jest ścieżką wtórną i nie może być
uznawany za pierwotną przyczynę.

## D2. Zachowane zachowanie

- Każdy dotychczasowy globalny fail-close pozostaje globalnym fail-close.
- Pierwszy close nadal zwiększa generation i counter dokładnie raz;
  idempotentne kolejne close nie zmieniają pierwszego reason ani nie emitują
  drugiego markera close.
- Prawdziwy poisoned mutex nadal kończy się `available=false` i atomowym
  zamknięciem admission; stan mutexu nie jest odzyskiwany.
- `CandidateAliasConflict` nie zmienia zachowania. Nie dodano żadnego nowego
  wyjątku, permitu ani ścieżki recovery.
- Nie zmieniono ACE, Brain, Gatekeepera, EventWritera, Triggera, PR2,
  konfiguracji rollout ani shadow/live semantics.

## D3. Dodatkowy kontekst stage failure

Przed wywołaniem recovery signal i przed wtórnym
`close(..., "receipt_stage_failed")`, log:

```text
Seer: canonical apply receipt staging failed; canonical runtime emission blocked
```

zapisuje snapshot `candidate_admission_open`, `registry_available`,
`admission_generation` i `authority_epoch_id`. Pozwala to odróżnić stan już
zamknięty od dostępnego registry bez zmiany decyzji runtime.

## D4. Focused proof i następny krok

Focused testy dowodzą, że pierwszy normalny close ma jeden marker i zachowuje
reason, a rzeczywiście poisoned `Mutex` daje osobny marker, zamknięte
admission i unavailable registry. Istniejące testy `alias_conflict` pozostają
bez zmian i muszą przejść.

Po zbudowaniu tej dokładnie diagnostycznej rewizji dozwolony jest tylko świeży
smoke diagnostyczny 120–600 s (lub do pierwszego close), z nowym run ID,
nowymi ścieżkami, `RUST_BACKTRACE=1` i zapisanym stderr. Nie jest to
qualifying smoke ani Day 1. Następnym artefaktem ma być pojedyncza
klasyfikacja pierwszej przyczyny; żadna naprawa `CandidateAliasConflict` nie
jest jeszcze autoryzowana.

> Uwaga o szablonie: ścieżka wymieniona w instrukcji globalnej,
> `/Gho/docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie.
> Dokument zachowuje lokalny format ADR-8D z `docs/ADR/`.
