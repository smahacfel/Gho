# GHOST — ZŁOTA ZASADA WYKONAWCZA

## CEL NADRZĘDNY

Ghost ma osiągnąć realny edge i dodatnie EV przez obserwację, filtrację, predykcję, selekcję lub scoring zgodne z istniejącą architekturą. Każde zadanie, plan i PR musi prowadzić bezpośrednio do tego celu albo usuwać konkretną przeszkodę, która go blokuje.

## ZŁOTA ZASADA

**Identyfikuj punkt zapalny i root cause. Wprowadzaj najmniejszą zmianę, która realnie rozwiązuje problem i daje mierzalny rezultat. Nie buduj systemu wokół problemu, gdy wystarcza jego bezpośrednia naprawa.**

Ta zasada ma pierwszeństwo przed proceduralną kompletnością, architektoniczną elegancją, przyszłościową elastycznością, audytową ceremonialnością i hipotetycznymi potrzebami, których bieżący cel nie wymaga.

## BEZWZGLĘDNE REGUŁY

1. Każdy plan zaczyna się od czterech zdań: **cel, root cause, najmniejsza skuteczna zmiana, minimalny dowód powodzenia**.
2. Każdy krok musi mieć bezpośrednie przełożenie na wynik zadania. Krok przygotowujący wyłącznie następny krok jest niedopuszczalny, chyba że jest technicznie niezbędny do uruchomienia zmiany.
3. Każdy PR musi samodzielnie dostarczać konkretną funkcję, naprawę lub mierzalny wynik. Zakazane są PR-y służące wyłącznie dopuszczeniu kolejnego PR-a, ceremonialne PR0 oraz podziały PRXA/PRXB/PRXC bez niezależnej wartości.
4. Dokumentacja ma być krótka i konkretna: decyzja, powód, zmienione zachowanie, ryzyko, sposób weryfikacji. Bez wielotysięcznych planów, narracji i powtarzania tych samych kontraktów.
5. Testy i kontrole ograniczają się do minimum, które dowodzi osiągnięcia celu oraz braku regresji w działającym zachowaniu. Nie buduj platformy testowej ani certyfikacyjnej bez bezpośredniej potrzeby produktu.
6. Infrastruktura pomocnicza powstaje tylko wtedy, gdy jest konieczna do wdrożenia, zmierzenia lub bezpiecznego użycia bieżącej zmiany. „Może przydać się później” nie jest uzasadnieniem.
7. Jedyną obowiązkową warstwą bezpieczeństwa jest ochrona działającego systemu przed regresją, uszkodzeniem aktywnej ścieżki, utratą danych lub niezamierzoną zmianą decyzji. Procedura nie jest celem.
8. Rozszerzenie zakresu poza bezpośrednią naprawę wymaga jawnego przedstawienia: dodatkowego kosztu, natychmiastowej wartości i prostszej alternatywy. Bez wyraźnej zgody właściciela zakres nie może zostać rozszerzony.
9. Gdy rozwiązanie zaczyna tworzyć rejestry, profile, sidecary, osobne protokoły, wieloetapowe audyty, bundle, burn-iny albo nowe frameworki, agent ma zatrzymać pracę i ponownie wykazać, że każdy element jest konieczny do bieżącego celu.
10. Koszt utopiony nigdy nie uzasadnia merge'u. Niedokończoną lub przeskalowaną pracę należy ciąć, upraszczać albo porzucać, jeśli nie daje proporcjonalnej wartości.

## OBOWIĄZKOWY KSZTAŁT PLANU

```text
CEL
ROOT CAUSE
NAJMNIEJSZA SKUTECZNA ZMIANA
PLIKI/OBSZAR
MINIMALNY TEST BRAKU REGRESJI
MIERNIK SUKCESU
POZA ZAKRESEM
```

Plan, którego nie da się przedstawić w tym formacie krótko i jednoznacznie, jest zbyt szeroki albo źle zdefiniowany.

## REGUŁA STOP

Agent ma natychmiast zatrzymać eskalację zakresu, gdy:

- praca pomocnicza zaczyna dominować nad zmianą właściwą;
- dokumentacja lub testy stają się osobnym produktem;
- plan przestaje kończyć się realną zmianą zachowania lub mierzalną wiedzą;
- pojawia się wieloetapowe „dopuszczenie do dopuszczenia”;
- rozwiązanie optymalizuje audytowalność zamiast edge/EV;
- koszt nie jest proporcjonalny do wpływu na cel.

W takim przypadku domyślną decyzją jest redukcja do najkrótszej ścieżki prowadzącej do rezultatu.
