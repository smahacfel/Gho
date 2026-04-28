# ADR-0065: PR4 runtime integration gate before PR5

**Date:** 2026-03-30
**Status:** Accepted
**Author:** Ghost Father

## Context

Po publikacji `ADR-0054` pozostała niebezpieczna dwuznaczność w języku wykonawczym dotyczącym relacji między `PR3B`, `PR4` i `PR5`.

Problem nie dotyczył statusu forensycznego modułów, lecz sposobu, w jaki kolejność mogła zostać przeczytana przez implementerów:

- `PR4` zostało opisane jako „sanity/integration pass”,
- w tabeli egzekucyjnej miało status „Warunkowo”,
- a `PR5` było opisane jako „pierwszy pełny etap po PR3B”.

Taka kombinacja sformułowań zostawiała nieakceptowalny luz interpretacyjny i mogła zostać odczytana tak, jakby `PR4` było krokiem opcjonalnym lub miękkim, zamiast obowiązkową bramką integracyjną.

To jest sprzeczne z rzeczywistą zależnością techniczną planu `PLANS/REFACTOR.md`, ponieważ `PR5`, `PR6`, `PR7` i `PR8` zakładają, że runtime session hot-path już poprawnie karmi warstwę `TxIntelligenceEngine`.

## Decision

Przyjmuje się jednoznaczny kontrakt wykonawczy:

**Obowiązkowa kolejność zależnościowa to:**

1. `PR3B` — runtime session cutover
2. `PR4` — **obowiązkowa runtime integration gate**
3. `PR5` — checkpoint/materialization wiring
4. `PR6` — feature-driven policy cutover
5. `PR7` — canonical truth migration off `ShadowLedger`
6. `PR8` — final cleanup legacy runtime

### Twarda interpretacja PR4

`PR4`:
- **nie jest opcjonalny**, 
- **nie jest wyłącznie „sanity passem” do odhaczenia opisowo**, 
- **nie jest restartem całego workstreamu modułowego**,
- jest **blokującą bramką wejścia do PR5**.

### Pass criteria dla bramki PR4

`PR4` jest zaliczone wyłącznie wtedy, gdy **wszystkie** poniższe warunki są spełnione jednocześnie:

1. każda transakcja obsługiwana przez runtime sesyjny przechodzi przez `TxIntelligenceEngine.on_transaction()`;
2. `TxIntelFeatures` są realnie odświeżane w trakcie obserwacji runtime, a nie tylko istnieją jako artefakt modułu lub testów;
3. bounded retention pozostaje bounded (`VecDeque` + `DEFAULT_SESSION_TX_RING_CAPACITY` albo równoważny jawny cap);
4. `TxIntelligenceEngine` nie uzyskuje bezpośredniego dostępu do canonical state;
5. testy `ghost-launcher/tests/tx_intelligence_tests.rs` pozostają zielone;
6. nie istnieje skok wykonawczy bezpośrednio z `PR4` do `PR6` z pominięciem `PR5`.

### Twarda reguła blokująca

**PR5 jest zabroniony, dopóki bramka PR4 nie przejdzie wszystkich pass criteria.**

Analogicznie:

- sekwencja `PR3B -> PR5` jest niepoprawna,
- sekwencja `PR3B -> PR4 -> PR6` jest niepoprawna,
- sekwencja `PR3B -> PR4 -> PR6 -> PR7 -> PR8` jest niepoprawna,

ponieważ wszystkie te warianty naruszają wymaganą zależność `PR4 -> PR5 -> PR6`.

### Relacja do ADR-0054

`ADR-0054` zachowuje ważność jako:
- macierz statusu forensycznego PR1–PR8,
- checklista operacyjna dla domykania hot-path.

Natomiast od teraz wszelkie fragmenty `ADR-0054`, które można było odczytać jako zmiękczenie obowiązkowości `PR4`, należy czytać przez pryzmat niniejszego ADR.

SSOT wykonawcze brzmi:

> `PR4` jest obowiązkową bramką integracyjną po `PR3B` i blokującym prerekwizytem `PR5`.

## Architectural Impact

To doprecyzowanie porządkuje relację między warstwami runtime:

- `PR3B` ustanawia sesję jako runtime owner,
- `PR4` potwierdza, że sesja realnie karmi behavioral layer,
- `PR5` może wejść dopiero wtedy, gdy behavioral layer działa w produkcyjnym hot-path,
- `PR6` nie może uczciwie przejść na feature-driven policy bez działającego `PR5`,
- `PR7` i `PR8` nie mogą być nazywane zamkniętymi, jeśli wcześniejsze zależności nie zostały spełnione.

To zmniejsza ryzyko budowania kolejnych etapów na martwych lub tylko częściowo zintegrowanych artefaktach.

## Risk Assessment

**Risk:** High

Jeśli `PR4` pozostaje opisane niejednoznacznie, skutki są poważne:

- zespoły mogą rozpocząć `PR5` bez realnego runtime feed do `TxIntelligenceEngine`,
- zespoły mogą błędnie przeskoczyć z `PR4` do `PR6`,
- późniejsze audyty mogą oceniać `PR6`/`PR7`/`PR8` na podstawie fałszywie założonej integracji behavioral layer,
- repo może wyglądać na „postępowe”, ale runtime pozostanie architektonicznie niespójny.

## Consequences

### Co staje się łatwiejsze

- jednoznaczne przekazywanie kolejności wykonawczej zespołowi;
- egzekwowanie bramek wejścia/wyjścia bez interpretacyjnych skrótów;
- zatrzymanie fałszywego postępu polegającego na przeskakiwaniu etapów zależnych.

### Co staje się trudniejsze

- nie da się już „skrótowo” opisać `PR4` jako lekkiego sanity passu;
- każdy zespół musi udowodnić realne przejście behavioral integration gate zanim ruszy dalej.

## Alternatives Considered

### 1. Pozostawić `ADR-0054` bez zmian i polegać na ustnym doprecyzowaniu

Rejected, ponieważ ta sama dwuznaczność wróciłaby przy kolejnym wykonawcy.

### 2. Opisać `PR4` jako pełny restart niezależnego workstreamu

Rejected, ponieważ status forensyczny nadal wskazuje, że sam moduł PR4 jest w dużej mierze dowieziony; problem dotyczy obowiązkowej integracji runtime, a nie konieczności restartu implementacji od zera.

### 3. Ustanowić `PR4` jako blokującą bramkę między `PR3B` i `PR5`

Accepted, ponieważ to jest jedyna interpretacja zgodna jednocześnie z rzeczywistą zależnością techniczną planu i z potrzebą wykonawczą bez luzu interpretacyjnego.

## Validation Steps

1. Przeanalizowano `PLANS/REFACTOR.md` pod kątem zależności `PR3B -> PR4 -> PR5 -> PR6`.
2. Przeanalizowano `ADR-0054` i zidentyfikowano sformułowania pozostawiające luz interpretacyjny (`Warunkowo`, `sanity/integration pass`, `pierwszy pełny etap po PR3B`).
3. Ustalono, że problem nie dotyczy statusu istnienia modułu PR4, lecz obowiązkowości jego runtime integration gate.
4. Zapisano jednoznaczny kontrakt wykonawczy blokujący rozpoczęcie `PR5` przed pełnym zaliczeniem `PR4`.
5. Zaktualizowano `ADR-0054`, tak aby nowa interpretacja była spójna z istniejącą macierzą forensyczną.
