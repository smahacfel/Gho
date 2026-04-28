# ADR-0114: Rust Master Production Skill Introduction

**Date:** 2026-04-26
**Status:** Accepted
**Author:** Codex 5.3

## Task Goal
Wprowadzenie nowego skilla `rust-master` jako centralnego kontraktu jakości dla produkcyjnego developmentu Rust w tym repozytorium.

## Summary of Work
- Dodano nowy skill w `.cursor/skills/rust-master/SKILL.md`.
- Zdefiniowano pełną doktrynę inżynierską obejmującą: kontrakty typów, granice API, deterministykę, dyscyplinę testów, `unsafe` policy, async/concurrency discipline, obserwowalność i koszt abstrakcji.
- Zachowano format metadata zgodny z pozostałymi skillami (`name`, `description`, `allowed-tools`).

## Decision Context
Repozytorium rozwija systemy o wysokiej wrażliwości na poprawność i regresje (Rust + Solana + trading orchestration). Dotychczas brakowało dedykowanego, produkcyjnego skilla Rust o jednoznacznych zasadach dla:
- type-driven contracts i granic modułowych,
- jawnej taksonomii błędów,
- walidowalnych reguł concurrency/async,
- deterministycznego zachowania i testowania ścieżek awarii,
- minimalizacji ryzyka wynikającego z `unsafe` i niekontrolowanych abstrakcji.

## Decision
Przyjęto nowy skill `rust-master` jako domyślną warstwę jakości dla zadań Rust. Skill:
1. Ustanawia regułę „correctness first, then performance, then ergonomics”.
2. Wymusza explicit error handling (bez `unwrap/expect` w ścieżkach produkcyjnych).
3. Traktuje API publiczne i typy jako kontrakty oraz wymaga testowania kontraktów.
4. Wprowadza twarde zasady dla `unsafe`, async runtime discipline i granic FFI.
5. Definiuje failure modes do aktywnego wykrywania oraz checklistę finalnego review.

## Alternatives Considered
1. **Brak nowego skilla, tylko reguły globalne**
   - Odrzucono: reguły globalne są zbyt ogólne i nie tworzą operacyjnej checklisty dla codziennych zadań Rust.
2. **Skrócona wersja skilla (minimalna)**
   - Odrzucono: nie pokrywałaby w pełni krytycznych obszarów (`unsafe`, FFI, determinism, test layers).
3. **Włączenie treści do istniejącego skilla Solana**
   - Odrzucono: miesza odpowiedzialności domenowe i utrudnia ponowne użycie dla zadań Rust niezależnych od Solany.

## Consequences
- **Pozytywne:** spójniejsza jakość implementacji Rust, mniejsze ryzyko regresji semantycznych i łatwiejsze code review.
- **Pozytywne:** wyraźne granice handoffu do innych skilli domenowych (`solana-pumpfun-architect`, `trading-systems`, `large-data-analytics`, `abstract-reasoning`).
- **Koszt:** większa rygorystyczność może wydłużyć czas implementacji, ale redukuje koszt błędów produkcyjnych.

## Architectural/Functional Changes Introduced
- Dodano nowy artefakt architektoniczny: `.cursor/skills/rust-master/SKILL.md`.
- Ustanowiono formalny kontrakt jakości dla ścieżek Rust-first w repo.

## Validation Steps
1. Zweryfikowano obecność pliku `SKILL.md` pod `.cursor/skills/rust-master/`.
2. Zweryfikowano poprawność frontmatter (`name`, `description`, `allowed-tools`).
3. Zweryfikowano spójność zakresu skilla z wymaganiami produkcyjnymi użytkownika.
