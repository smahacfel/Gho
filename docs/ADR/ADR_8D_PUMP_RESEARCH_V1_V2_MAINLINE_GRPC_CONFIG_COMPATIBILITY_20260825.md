# ADR-8D: Pump Research Tape V1/V2 — zgodność standalone gRPC z `main`

**Data:** 2026-08-25

## Kontekst

Gałąź integracyjna Tape V1/V1.1 i prospective Tape V2 jest budowana od
aktualnego `main`. Jej standalone konstruktor
`PumpResearchSourceConnectionV1::new_for_subscription_profile` tworzy
`GrpcConfig` wyłącznie dla profili researchowych.

W źródłowym worktree implementacji istniało dodatkowe pole
`scope_primary_global_account_updates_to_registry`. Pole pochodziło z
niezależnej, nieobecnej na `main`, pracy nad aktywnym profilem
`PrimaryGlobal`. Integracyjny `GrpcConfig` na `main` tego pola nie posiada,
a jego nieobecność oznacza istniejące zachowanie `false` — brak registry scope.

## Problem

Standalone konstruktor ustawiał literalnie:

```rust
scope_primary_global_account_updates_to_registry: false,
```

Po integracji od `main` powodowało to błąd kompilacji `E0560`, zanim możliwa
była weryfikacja Tape V1/V2. Dodanie całego niepowiązanego kontraktu aktywnego
`PrimaryGlobal` tylko po to, aby dostarczyć pole o wartości `false`, rozszerzyłoby
zakres PR i zmieniłoby runtime poza celem research tape.

## Decyzja

Usuwamy wyłącznie inicjalizator pola z research-only konstruktora.

To zachowuje literalną semantykę standalone capture:

```text
registry scope = disabled
```

ponieważ `main` nie implementuje takiego scope w ogóle. Nie dodajemy pola,
metryk, filtrów ani metod z niezależnej gałęzi aktywnego runtime'u.

## Konsekwencje i granice

- V1/V1.1 i PRXTAPE3 V2 zachowują oddzielny, standalone profil source capture.
- Nie zmieniono `SeerConfig`, aktywnego `PrimaryGlobal`, Event Bus,
  OracleRuntime, Gatekeepera ani execution.
- Nie zmieniono źródeł raw, semantics, kwalifikatora, eksportera,
  denominatora ani zasad exact-state.
- Zmiana jest wymagana wyłącznie dla kompilowalności tej samej semantyki na
  bazie `main`.

## Granica attestation istniejącego artefaktu

Ta integracyjna zmiana źródłowa powoduje nowy digest release executable.
Zachowany artifact exact-state kwalifikacji pozostaje immutable i jest
poprawnie przypięty do historycznego executable użytego przy kwalifikacji;
nie wolno podmieniać tego executable binarką zbudowaną z tej gałęzi tylko na
podstawie równoważności semantyki.

PR nie zmienia raw, receiptów ani exact outputu i nie jest retroaktywną
rekwalifikacją. Jeżeli potrzebny będzie artifact przypięty do binarki z
`main`, wymaga to osobnej, jawnie autoryzowanej offline qualification tego
samego immutable raw i nowego create-new outputu.

## Weryfikacja

Po zmianie pełna locked/offline macierz V1/V1.1 i V2 jest uruchamiana w
czystym worktree integracyjnym. W szczególności wymaga przejścia:

```text
cargo check --locked --offline -p seer --bin pump-exact-state-tape-v2
cargo test --locked --offline -p seer grpc_connection::tests --lib --no-fail-fast
cargo test --locked --offline -p seer research_tape --lib --no-fail-fast
cargo test --locked --offline -p seer research_exact_tape_v2 --lib --no-fail-fast
```

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
