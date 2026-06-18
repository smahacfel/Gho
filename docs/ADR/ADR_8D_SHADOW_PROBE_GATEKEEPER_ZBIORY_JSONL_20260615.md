# ADR-8D: Skrypt do laczenia shadow/probe lifecycle z gatekeeper decisions i podzialu po final_pnl_pct

Status: wykonane
Typ: implementacja lokalnego narzedzia analitycznego
Data: 2026-06-15
Repo/branch: /root/Gho / codex/gatekeeper-edge-policy-redesign-r1
Commit/PR: n/a
Zakres: dodanie nowego skryptu Python w `scripts/` wraz z testem kontraktowym
Dotkniete moduly/pliki: `scripts/zbiory.py`, `scripts/test_zbiory.py`
Powiazane runy/logi/raporty: n/a
Poziom ryzyka: niski

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
- sprawdzic istniejace skrypty laczace lifecycle i separator JSONL
- dodac jeden nowy skrypt dzialajacy na aktualnym katalogu roboczym
- dodac waski test end-to-end i nie dotykac hot path runtime

Rzeczywisty przebieg:
- przejrzano `scripts/merge_lifecycles.py` i `scripts/separator.py` jako lokalne punkty odniesienia
- zaimplementowano nowy skrypt `scripts/zbiory.py` bez ingerencji w istniejace narzedzia
- dodano `scripts/test_zbiory.py` do walidacji joinu, deduplikacji i podzialu wyjscia

Odchylenia od planu:
- brak odchylen

## 2. Wykorzystane skills/sub-agenci

Nazwa:
- brak

Powod uzycia:
- task byl lokalny i nie dotykal aktywnych kontraktow runtime Ghost

Zakres uzycia:
- n/a

Wynik:
- n/a

Ograniczenia:
- brak

## 3. Opis problemu — 3W2H

What:
- potrzebny byl skrypt, ktory w katalogu roboczym znajdzie `shadow_lifecycle.jsonl`, `probe_shadow_lifecycle.jsonl` i `gatekeeper_v2_decisions.jsonl`, scali rekordy po mint i rozdzieli wynik na trzy zbiory po `final_pnl_pct`

Where:
- narzedzia analityczne w `scripts/`

Why it matters:
- reczne skladanie tych zbiorow jest powtarzalne, podatne na pomylki i nie daje stalego kontraktu wyjsciowego

How observed:
- bezposrednie wymaganie uzytkownika

How many / scale:
- jeden nowy skrypt i jeden test

Evidence:
- implementacja w `scripts/zbiory.py`
- walidacja w `scripts/test_zbiory.py`

## 4. Przyczyna zrodlowa

Root cause:
- repo nie mialo jednego, prostego skryptu realizujacego caly pipeline: filtr lifecycle -> join z decisions -> podzial na A/B/N

Mechanizm bledu:
- funkcjonalnosc byla rozbita na osobne, czesciowo pokrywajace sie narzedzia

Miejsce:
- warstwa skryptow analitycznych

Skutek:
- koniecznosc laczenia kilku krokow recznie lub adaptowania innych skryptow

Dowod:
- istniejace `merge_lifecycles.py` i `separator.py` pokrywaly tylko fragmenty wymagania

Odrzucone hipotezy:
- brak potrzeby modyfikacji runtime Rust
- brak potrzeby zmiany log schema albo DecisionLogger

## 5. Strategia naprawy

Przyjeta strategia:
- dodac nowy, samowystarczalny skrypt Python dzialajacy domyslnie na `Path.cwd()`

Zakres ingerencji:
- tylko `scripts/` oraz dokumentacja ADR-8D

Czego nie zmieniano:
- runtime Ghost
- Gatekeeper policy
- log schema JSONL
- istniejace skrypty pomocnicze

Ryzyka:
- przy wielu rekordach decisions dla jednego `base_mint` skrypt emituje wiele scalonych rekordow dla tego samego mint
- duplikaty `mint_id` w lifecycle sa deduplikowane przez zasade "first seen wins"

Odrzucone alternatywy:
- przerobienie `merge_lifecycles.py`
- przerobienie `separator.py`
- broad refactor narzedzi analitycznych

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `scripts/zbiory.py`
- Co zmieniono: dodano nowe CLI do skanowania 3 plikow JSONL, deduplikacji lifecycle po `mint_id`, joinu z decisions po `base_mint` i zapisu `zbior_A.jsonl`, `zbior_B.jsonl`, `zbior_N.jsonl`
- Dlaczego: zrealizowac caly wymagany workflow w jednym miejscu
- Efekt: mozna uruchomic np. `python3 /root/Gho/scripts/zbiory.py 30 -30`

Zmiana 2:
- Plik/modul: `scripts/test_zbiory.py`
- Co zmieniono: dodano test end-to-end dla merge i podzialu wyjsc
- Dlaczego: potwierdzic kontrakt funkcjonalny bez odpalania calego repo
- Efekt: szybka walidacja lokalna

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Unit | `python3 -m unittest scripts/test_zbiory.py` | test przechodzi | PASS | lokalny test kontraktowy |
| Guard negative case | deduplikacja `mint-dup`, odrzucenie `mint-no-pnl`, odrzucenie `mint-unmatched` | zgodne z oczekiwaniem | PASS | asercje w `scripts/test_zbiory.py` |

Wniosek walidacyjny:
- skrypt realizuje wymagany join i podzial dla reprezentatywnego zestawu danych

Ograniczenia walidacji:
- nie wykonano testu na produkcyjnych wolumenach logow

## 8. Wdrozene zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: test kontraktowy
- Co zabezpiecza: poprawne mapowanie `mint_id` -> `base_mint`
- Kiedy sie aktywuje: przy lokalnym uruchomieniu testu
- Jak przetestowano: `python3 -m unittest scripts/test_zbiory.py`
- Co pozostaje poza zakresem: wydajnosc dla bardzo duzych plikow

Guardrail 2:
- Typ: jawne provenance fields
- Co zabezpiecza: audytowalnosc pochodzenia rekordow po merge
- Kiedy sie aktywuje: przy zapisie kazdego scalonego rekordu
- Jak przetestowano: asercje na `_lifecycle_source_file` i `_decision_source_file`
- Co pozostaje poza zakresem: dodatkowe metadane o wszystkich odrzuconych rekordach

## Otwarte ryzyka / follow-up

- jesli potrzebna bedzie inna polityka duplikatow niz `first seen wins`, trzeba ja doprecyzowac i dopisac osobny parametr CLI
- jesli potrzebny bedzie rowniez plik pelnego merged output, mozna go dodac jako opcjonalny artefakt bez zmiany obecnego kontraktu A/B/N
