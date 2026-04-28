# ADR-0085: Launcher trigger config contract restoration

**Date:** 2026-04-07
**Status:** Accepted
**Author:** Ghost Father

## Context

`ghost-launcher/src/config.rs` uległ strukturalnej korupcji podczas wcześniejszych zmian związanych z kontraktem live Jito transport. Fragment logiki walidacyjnej został wstrzyknięty do wnętrza definicji `TriggerComponentConfig`, a nagłówek `TriggerShadowRunConfig` został częściowo usunięty.

Efektem był błąd parsera Rusta `unexpected closing delimiter: }`, utrata części pól konfiguracji triggera oraz rozjazd między implementacją a testami. Dodatkowo walidacja live transport zaczęła błędnie wymagać `jito_uuid`, mimo że aktualny kontrakt systemu dopuszcza brak UUID dla transportu live, przy jednoczesnym fail-closed dla wartości placeholderowych.

## Decision

Przywrócono pełny kontrakt struktur konfiguracyjnych w `ghost-launcher/src/config.rs`:

- odtworzono brakujące pola `TriggerComponentConfig`
- odtworzono definicję `TriggerShadowRunConfig`
- usunięto omyłkowo wklejony kod walidacyjny z wnętrza definicji structa
- przywrócono semantykę walidacji live transport:
  - `jito_uuid` jest opcjonalny
  - jeśli jest podany, wartości placeholderowe pozostają zabronione

## Architectural Impact

Ta decyzja przywraca zgodność między:

- SSOT konfiguracji launchera
- testami walidującymi execution profile
- kontraktem live Jito transport w triggerze
- rollout profiles używanymi przez środowiska live i shadow

Wpływ obejmuje cały pipeline bootstrapu konfiguracji launchera, ponieważ `TriggerComponentConfig` jest wczytywany i walidowany przed startem runtime.

## Risk Assessment

**Rate:** Medium

Ryzyka regresji dotyczą głównie:

- ponownego uszkodzenia layoutu struktur konfiguracyjnych przy ręcznych merge'ach
- przypadkowego powrotu do twardego wymagania `jito_uuid`
- cichego rozjazdu między domyślną konfiguracją, walidacją i testami

Nie ma zmiany layoutu danych runtime poza przywróceniem wcześniej oczekiwanego kontraktu źródłowego.

## Consequences

Pozytywne:

- `ghost-launcher` znowu kompiluje się poprawnie
- konfiguracja triggera odzyskuje kompletność
- live transport pozostaje zgodny z aktualnym kontraktem operacyjnym
- placeholder UUID nadal fail-closed

Negatywne:

- utrzymanie tego kontraktu wymaga dalszej dyscypliny przy ręcznych edycjach dużego pliku `config.rs`
- sam rozmiar pliku nadal zwiększa ryzyko podobnych błędów integracyjnych w przyszłości

## Alternatives Considered

### 1. Pozostawić `jito_uuid` jako wymagane dla live transport

Odrzucono, ponieważ łamałoby to aktualny kontrakt wdrożeniowy i testy akceptujące live transport bez UUID.

### 2. Naprawić wyłącznie klamry bez odtwarzania pełnego kontraktu pól

Odrzucono, ponieważ prowadziłoby to do kompilowalnego, ale semantycznie uszkodzonego modelu konfiguracji.

### 3. Natychmiastowy pełny refactor `config.rs` na mniejsze moduły

Odrzucono na tym etapie jako zbyt szeroki blast radius względem pilnej potrzeby przywrócenia poprawnej kompilacji i zgodności rolloutów.

## Validation Steps

- uruchomić test akceptujący live transport bez `jito_uuid`
- uruchomić test odrzucający placeholder `jito_uuid`
- wykonać `cargo build --release -p ghost-launcher`
- potwierdzić brak błędu parsera `unexpected closing delimiter`
- potwierdzić brak regresji w ładowaniu execution profile
