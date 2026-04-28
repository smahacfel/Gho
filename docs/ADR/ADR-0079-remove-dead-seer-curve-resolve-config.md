# ADR-0079: Remove dead Seer curve-resolve config

**Date:** 2026-04-03
**Status:** Accepted
**Author:** Ghost Father

## Context

ADR-0078 usunął martwą ścieżkę RPC resolve dla `curve -> mint` w Seerze, ale tymczasowo zostawił dwa compatibility leftovers:

- stałą `CURVE_RESOLVE_MAX_CONCURRENT`
- pole `SeerConfig::curve_resolve_max_concurrent`

Po dalszym przeglądzie okazało się, że te elementy nie mają już żadnego runtime consumer. Nie sterują żadnym semaphore, workerem ani resolverem. Pozostały wyłącznie jako atrapa w:

- `seer::config::SeerConfig`
- ręcznych builderach configu w `ghost-launcher` i `ghost-brain`
- testach/defaultach

To tworzyło błędne wrażenie, że Seer nadal posiada aktywny feature ograniczania współbieżnych resolve RPC, mimo że cała architektura resolve path została już usunięta.

## Decision

Usuwamy całkowicie martwy kontrakt konfiguracyjny związany z curve-resolve RPC:

- kasujemy stałą `CURVE_RESOLVE_MAX_CONCURRENT`
- kasujemy pole `SeerConfig::curve_resolve_max_concurrent`
- kasujemy helper `SeerConfig::default_curve_resolve_max_concurrent()`
- czyścimy wszystkie ręczne inicjalizacje `SeerConfig`, które ustawiały to pole
- czyścimy testy i komentarze sugerujące istnienie resolver semaphore / concurrency knob

Konfiguracje użytkownika, które nadal zawierają ten klucz, nie stanowią nowego kontraktu runtime — po usunięciu pola są po prostu nieobsługiwanym legacy leftovers i nie powinny być traktowane jako aktywny feature.

## Architectural Impact

Architektura Seera staje się jednoznaczna:

- nie istnieje już żaden RPC hot path dla `curve -> mint`
- mapping jest rejestrowany przez create/trade/account-update hooks
- nie istnieje żaden semaphore-controlled resolver do strojenia przez config

To upraszcza SSOT dla operatorów i developerów: jeśli ktoś widzi `SeerConfig`, nie dostaje już fałszywego sygnału, że resolver RPC można „podkręcić” wartością concurrency.

## Risk Assessment

**Rate:** Low

Ryzyko regresji jest niskie, bo usuwany kontrakt nie miał już żadnego wykonującego go runtime consumer.

Potencjalny wpływ uboczny:

- stare przykłady lub lokalne configi mogą nadal zawierać usunięty klucz,
- ręczne konstruktory `SeerConfig` w innych modułach mogły wymagać aktualizacji kompilacyjnej.

Oba ryzyka są jawne i łatwo wykrywalne podczas build/test.

## Consequences

### Positive

- `SeerConfig` przestaje reklamować feature, którego nie ma
- buildery w `ghost-launcher` i `ghost-brain` są prostsze
- testy nie utrwalają już fikcyjnej architektury resolvera
- kod i ADR-y są spójniejsze z rzeczywistym runtime

### Negative

- stare notatki/plan files mogą nadal historycznie wspominać ten knob
- operatorzy, którzy mieli taki klucz w lokalnym configu, stracą iluzję że cokolwiek nim sterują

## Alternatives Considered

### 1. Zostawić pole jako backward-compat noop

Odrzucone. No-op config jest gorszy niż brak configu, bo komunikuje nieistniejący feature i zachęca do błędnego debugowania.

### 2. Oznaczyć pole jako deprecated, ale zostawić

Odrzucone. To nadal utrzymuje fałszywy kontrakt architektoniczny i rozmywa granicę po ADR-0078.

### 3. Przywrócić realny resolver tylko po to, żeby pole miało sens

Odrzucone. Byłby to rollback w stronę wolniejszej i bardziej złożonej architektury, którą właśnie usunęliśmy jako martwą.

## Validation Steps

1. Zbudować/checknąć moduły używające `SeerConfig` i poprawić wszystkie ręczne inicjalizacje.
2. Uruchomić diagnostykę dla `seer`, `ghost-launcher` i `ghost-brain`.
3. Potwierdzić grepem, że w aktywnym kodzie nie ma już `curve_resolve_max_concurrent` ani `CURVE_RESOLVE_MAX_CONCURRENT`.
4. Zachować ADR-0078 jako historyczny zapis etapu przejściowego, ale wskazać ADR-0079 jako finalne domknięcie cleanupu.
