# ADR-0042: Paper broker fill ownership for concurrent paper lifecycles

**Date:** 2026-03-27
**Status:** Accepted
**Author:** Ghost Father

## Context

Podczas aktywnego paper burn-in jeden candidate utknął na ścieżce `Candidate -> EntrySubmitted` bez `EntryFilled`, `PositionOpened` i `PositionClosed`, mimo że runtime nadal żył.

Forensics wykazały konkretny układ zdarzeń:

- candidate `4MMBvoN6WR3cAcGbttpwxFAtQ4dxkSa1FBiQ3ee5JJbe_E4JgyyJvWJrPRuTTMrRuGKZeYm2sC6SchYMsdhogNpN5_1774649874933` dostał `EntrySubmitted` z `order_id=paper-3`,
- równolegle candidate `9SMAxMgTvdCZoA6pXu3eF3gkK82pTgfyvvuEFpUQpump_AgaNiBLXnCfzJVHaEdXq27YTrvRGRYExX3QzqdGJTgca_1774649875006` dostał `EntrySubmitted` z `order_id=paper-4`,
- broker otworzył obie pozycje (`paper-pos-2`, `paper-pos-3`),
- ale tylko jeden lifecycle zobaczył swój fill i wyemitował `PositionOpened`,
- drugi lifecycle po 5 s zakończył się ostrzeżeniem `PaperLifecycle: entry fill never arrived, aborting`.

Root cause nie leżał w guardzie closeoutu ani w operatorze. Problem był współbieżny i lokalny dla paper runtime:

- `PaperPositionLifecycle::run()` współdzieli jeden `PaperBroker`,
- każdy lifecycle wywoływał `broker.poll_fills(now_ms).await`,
- `poll_fills()` opróżniał globalną kolejkę dojrzałych orderów i zwracał **wszystkie** gotowe fille do taski, która wywołała go jako pierwsza,
- taska przetwarzała tylko fill własnego `order_id`, a pozostałe fille były tracone dla innych lifecycle.

## Decision

Wprowadzono jawną własność filla po `order_id` wewnątrz `PaperBroker`:

- broker utrzymuje teraz mapę `completed_fills`,
- dojrzałe ordery są rozliczane do `completed_fills`, zamiast być jednorazowo oddawane pierwszemu pollerowi,
- `PaperLifecycle` używa nowej metody `take_fill_for_order(order_id, now_ms)`, która pobiera wyłącznie fill należący do konkretnego orderu,
- ten sam mechanizm zastosowano zarówno do entry, jak i exit polling.

Dodatkowo doprecyzowano log ostrzeżenia `entry fill never arrived`, aby zawierał `candidate_id` i `order_id`.

## Architectural Impact

- `PaperBroker` przestaje zachowywać się jak globalny, destrukcyjny stream filli bez własności konsumenta.
- `PaperPositionLifecycle` zachowuje model współdzielonego brokera, ale nie może już kraść filli innym taskom.
- SSOT się nie zmienia: broker nadal jest jedynym miejscem rozliczenia paper orderów, a lifecycle nadal emituje pełen timeline eventów.

## Risk Assessment

**Rate:** Medium

- Zmiana dotyczy centralnej ścieżki symulowanych filli paper entry/exit.
- Ryzyko regresji dotyczy głównie kolejkowania i konsumpcji filli przy konkurencyjnych lifecycle.
- Ryzyko jest akceptowalne, bo poprzednie zachowanie udowodniono jako błędne na realnym runie, a fix zachowuje lokalny i deterministyczny zakres.

## Consequences

- Łatwiejsze: równoległe paper lifecycle nie mogą już wzajemnie gubić filli.
- Łatwiejsze: `paper_inflight` nie powinno już powstawać z powodu fill theft między taskami.
- Łatwiejsze: logi abortu wskazują konkretny candidate i order.
- Trudniejsze: broker ma teraz dodatkowy stan pośredni `completed_fills`, który trzeba utrzymywać spójnie.

## Alternatives Considered

1. **Osobny broker per lifecycle**
   - Odrzucone, bo rozbiłoby współdzielony limit pozycji i wspólną semantykę paper runtime.

2. **Kanał/pub-sub filli między brokerem a lifecycle**
   - Odrzucone jako większa przebudowa niż wymagała naprawa konkretnego incydentu.

3. **Pozostawić `poll_fills()` i zwiększyć timeout `entry fill never arrived`**
   - Odrzucone, bo to maskowałoby wyścig zamiast go usuwać.

## Validation Steps

1. Potwierdzić statycznie brak błędów w `ghost-brain/src/execution/paper.rs` i `ghost-brain/src/execution/paper_lifecycle.rs`.
2. Uruchomić nowy run paper-burnin i sprawdzić, że równoległe `EntrySubmitted` nie kończą się już osieroconym `Candidate -> EntrySubmitted`.
3. Zweryfikować nowy test regresyjny `test_concurrent_paper_lifecycles_do_not_steal_each_others_fills` przy najbliższej lekkiej sesji testowej lub po odzyskaniu headroomu dyskowego.
4. Potwierdzić, że guard closeoutu dochodzi do `SAFE_TO_STOP` bez stuck candidate wynikającego z fill theft.