# ADR-8D: FSC Volume Storage Plan Artifact

Status: Done
Typ: Documentation / plan artifact
Data: 2026-06-15
Repo/branch: /root/Gho
Commit/PR: none
Zakres: zapis zaakceptowanego planu do PLANS/PLAN_FSC_VOLUME.md
Dotknięte moduły/pliki:
- PLANS/PLAN_FSC_VOLUME.md
- docs/ADR/ADR_8D_FSC_VOLUME_PLAN_ARTIFACT_20260615.md
Powiązane runy/logi/raporty: R30 FSC/lookback diagnostics, wcześniejsze obserwacje storage FSC
Poziom ryzyka: low

## 1. Przygotowanie i działania wstępne

Plan początkowy: użytkownik poprosił o zapisanie treści planu FSC volume/TTL do nowego pliku `Gho/PLANS/PLAN_FSC_VOLUME.md` i wyraźnie zakazał realizacji planu.

Rzeczywisty przebieg: sprawdzono, że plik `PLANS/PLAN_FSC_VOLUME.md` nie istnieje, odczytano szablon ADR-8D, następnie utworzono plik planu oraz niniejszy ADR wymagany przez repozytoryjne zasady dokumentowania zmian.

Odchylenia od planu: brak. Nie wykonywano zmian runtime, configu, mountów, cleanupu ani uruchomień.

## 2. Wykorzystane skills/sub-agenci

Nazwa: none in this execution turn
Powód użycia: zadanie było lokalnym zapisem wcześniej przygotowanego planu.
Zakres użycia: nie dotyczy.
Wynik: nie dotyczy.
Ograniczenia: treść planu opiera się na wcześniejszej analizie aktywnego R30 configu i writerów NLN/FSC; ten ADR dokumentuje tylko zapis artefaktu.

## 3. Opis problemu — 3W2H

What: plan bezpiecznego przeniesienia FSC capture na dedykowany wolumen z rotowanymi segmentami i TTL cleanupem musiał zostać utrwalony jako artefakt repozytoryjny.

Where: `/root/Gho/PLANS/PLAN_FSC_VOLUME.md`.

Why it matters: FSC przy obecnym modelu capture może generować nieakceptowalny koszt storage; utrwalony plan zapobiega utracie kontekstu przed przyszłą implementacją.

How observed: z poprzednich runtime checków i dyskusji wynikało, że FSC clean coverage jest bardzo niskie, a koszt storage wysoki.

How many / scale: użytkownik wskazał szacunek rzędu około 1.8 TB / 12h po skalowaniu do wysokiego coverage.

Evidence: plan zawiera wymagane guardraile: FSC-only bind mount, segmentację, manifest, janitor, TTL >= lookback + safety buffer i brak cleanupu DecisionLogger/shadow lifecycle.

## 4. Przyczyna źródłowa

Root cause: brak utrwalonego planu implementacji storage guardrail dla FSC volume/TTL.

Mechanizm błędu: bez artefaktu plan mógłby zostać utracony lub odtworzony niespójnie podczas późniejszej naprawy FSC coverage.

Miejsce: dokumentacja planów w `PLANS/`.

Skutek: ryzyko późniejszej, niepełnej implementacji bez twardych zabezpieczeń przed usuwaniem potrzebnych danych.

Dowód: użytkownik poprosił o zapisanie planu do konkretnego pliku przed przełączeniem w tryb agenta.

Odrzucone hipotezy: nie implementowano runtime changes; to nie jest poprawka kodu ani configu.

## 5. Strategia naprawy

Przyjęta strategia: zapisać kompletny, decyzyjny plan jako markdown w `PLANS/PLAN_FSC_VOLUME.md`.

Zakres ingerencji: dokumentacja planu i ADR.

Czego nie zmieniano:
- kodu runtime,
- configów rollout,
- mountów systemowych,
- skryptów cleanup,
- aktualnego R30 procesu.

Ryzyka: minimalne; zmiana dokumentacyjna.

Odrzucone alternatywy: implementacja planu została odrzucona przez aktualną instrukcję użytkownika.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/moduł: `PLANS/PLAN_FSC_VOLUME.md`
- Co zmieniono: dodano kompletny plan FSC capture na dedykowanym wolumenie z segmentacją i janitorem.
- Dlaczego: użytkownik poprosił o utrwalenie planu bez implementacji.
- Efekt: plan jest dostępny jako trwały artefakt repozytoryjny.

Zmiana 2:
- Plik/moduł: `docs/ADR/ADR_8D_FSC_VOLUME_PLAN_ARTIFACT_20260615.md`
- Co zmieniono: dodano ADR-8D dokumentujący utworzenie planu.
- Dlaczego: repozytoryjne zasady wymagają ADR-8D po utworzeniu pliku projektu.
- Efekt: zmiana dokumentacyjna jest audytowalna.

## 7. Walidacja działań naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowód |
|---|---|---|---|---|
| Istnienie pliku docelowego | `test -f PLANS/PLAN_FSC_VOLUME.md && test -f docs/ADR/ADR_8D_FSC_VOLUME_PLAN_ARTIFACT_20260615.md` | oba pliki istnieja | PASS | `files_exist=PASS` |
| Markdown/diff sanity | `git diff --check -- PLANS/PLAN_FSC_VOLUME.md docs/ADR/ADR_8D_FSC_VOLUME_PLAN_ARTIFACT_20260615.md` | brak whitespace errors | PASS | exit code 0 |

Wniosek walidacyjny: zmiana jest dokumentacyjna i przeszla walidacje obecnosci plikow oraz `git diff --check`.

Ograniczenia walidacji: nie uruchamiano testów runtime, bo nie zmieniano kodu ani configu.

## 8. Wdrożone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: dokumentacyjny
- Co zabezpiecza: przyszła implementacja nie powinna kasować aktywnych FSC segmentów ani DecisionLogger/shadow lifecycle.
- Kiedy się aktywuje: przy późniejszym wdrażaniu planu.
- Jak przetestowano: nie dotyczy tej dokumentacyjnej zmiany.
- Co pozostaje poza zakresem: realna implementacja segmentacji, janitora i preflight.

Guardrail 2:
- Typ: scope boundary
- Co zabezpiecza: aktualne polecenie użytkownika `Nie realizuj planu`.
- Kiedy się aktywuje: w tej turze pracy.
- Jak przetestowano: brak zmian kodu/runtime/configu.
- Co pozostaje poza zakresem: przyszłe wykonanie planu po odrębnej akceptacji.

## Otwarte ryzyka / follow-up

- Przyszła implementacja musi dobrać TTL po naprawie FSC coverage i realnym pomiarze lookbacku.
- Przyszła implementacja musi utworzyć osobny ADR-8D obejmujący zmiany kodu, configu i operacyjny rollout.
- Przed wdrożeniem bind mount trzeba potwierdzić docelowy wolumen, source mount i minimalny budżet wolnego miejsca.
