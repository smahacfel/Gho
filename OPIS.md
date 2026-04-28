# GHOST — KOMPLEKSOWA RECENZJA PROJEKTU
### Dla każdego, kto nigdy nie miał kontaktu z kryptowalutami

---

## O CO W TYM WSZYSTKIM CHODZI — PROSTO I SZCZERZE

Wyobraź sobie giełdę, na której co kilka sekund pojawia się nowy sklep. Nikt nie wie, czy przetrwa godzinę, czy stanie się imperium handlowym. Większość nowo otwartych sklepów to oszustwa lub totalne klapy. Ale co jakiś czas — może jeden na dwadzieścia — pojawia się coś prawdziwego, z organicznym ruchem klientów i realnym potencjałem wzrostu. Ktokolwiek wejdzie do takiego sklepu jako jeden z pierwszych klientów i kupi coś wartościowego, może kilka minut później sprzedać to za wielokrotność zapłaconej ceny.

**Ghost** to automatyczny system, który — działając szybciej niż jakikolwiek człowiek — obserwuje tę "giełdę", odsiewa 95% śmiecia i wchodzi tylko w ten jeden sklep na dwadzieścia, zanim zrobi to konkurencja.

Konkretnie: Ghost działa na blockchainie **Solana** (Solana to publiczny rejestr transakcji, coś w rodzaju notariusza dla pieniędzy cyfrowych — rejestruje każdą zmianę własności w ułamkach sekundy). Monitoruje platformę **Pump.fun** i **PumpSwap** — miejsca, gdzie co chwilę ktoś tworzy nowy token (coś jak nowa waluta wymyślona przez kogoś). Ghost analizuje każdy taki nowy token w ciągu setek milisekund i decyduje: wchodzę czy odpuszczam.

Jego przewaga nie polega na tym, że jest mądrzejszy od rynku. Polega na tym, że **działa szybciej i analizuje więcej danych jednocześnie** niż jakakolwiek osoba siedząca przed ekranem.

---

## SKĄD POCHODZI WIEDZA SYSTEMU — UCHO SYSTEMU (SEER)

Zanim Ghost cokolwiek zdecyduje, musi wiedzieć, co się dzieje. Tę rolę pełni moduł o nazwie **Seer** (ang. "jasnowidz") — ciągłe, wielokanałowe ucho systemu.

### Trzy źródła danych jednocześnie

**1. GRPC Yellowstone (Chainstack)**
To najszybsze i najbardziej wiarygodne źródło. Wyobraź sobie bezpośredni telefon do operatora giełdy — Ghost dostaje surowe dane z blockchainu dosłownie kilka milisekund po tym, jak transakcja zostaje potwierdzona. Dane oznaczane są jako `RawChain` (surowy łańcuch) — najwyższy poziom wiarygodności.

**2. WebSocket Geyser / Helius**
Alternatywne połączenie przez pośrednika — nieco wolniejsze, ale redundantne. Dane z tego źródła oznaczane są jako `AdaptedChain` (zaadaptowany łańcuch) — pośredni poziom wiarygodności.

**3. PumpPortal API**
Dedykowane API platformy Pump.fun, które dostarcza dodatkowych danych o nowych tokenach (nazwa, symbol, wstępna płynność). Dane oznaczane są jako `Synthetic` — traktowane jako pomocnicze, nie jako główne źródło prawdy.

Każda informacja docierająca do systemu **niesie ze sobą metadane jakości** — skąd pochodzi, czy mamy pewny numer bloku (slotu), czy timestamp pochodzi z blockchainu czy z zegara systemowego. Dzięki temu, gdy dwie informacje z różnych źródeł są ze sobą sprzeczne, system wie, której ufać bardziej.

### Co Seer robi z surowymi danymi?

Seer odczytuje surowe transakcje w formacie binarnym (ciągi bajtów) i je "rozkodowuje". Każdy program na Solanie ma swój unikalny **discriminator** — coś w rodzaju odcisku palca zakodowanego na początku instrukcji. Ghost rozpoznaje na przykład:
- `0x18 0x64 0xAA 0x3B` → Pump.fun: stworzono nowy token
- `0x66 0x06 0x3D 0x61` → Pump.fun: ktoś kupuje
- `0x33 0xE6 0x85 0xA3` → Pump.fun: ktoś sprzedaje

Po rozpoznaniu rodzaju transakcji Seer wyciąga z niej kluczowe dane: adres puli (unikalny identyfikator nowego tokena), kto to zrobił, ile SOL wydał, ile tokenów dostał. Całość ląduje w wewnętrznej kolejce — **Event Bus**.

### Event Bus — autostrada wewnętrzna

Event Bus to wewnętrzna szyna komunikacyjna z buforem na **10 240 zdarzeń**. Dlaczego akurat tyle? Na Solanie w szczycie aktywności przelewa się 2–3 tysiące transakcji na sekundę. Przy 100–200 aktywnie śledzonych tokenach system generuje dziesiątki zdarzeń na sekundę. Bufor 10 240 daje około 5–10 sekund "oddechu" — żaden komponent systemu nie traci zdarzeń, nawet jeśli przez chwilę przetwarza coś kosztownego obliczeniowo.

---

## SHADOW LEDGER — PAMIĘĆ SYSTEMU, CENNIK W CZASIE RZECZYWISTYM

Zanim Ghost zdecyduje, czy kupić token, musi wiedzieć, ile on kosztuje i jak bardzo zakup wpłynie na cenę. Na normalnej giełdzie pytasz o kurs i dostajesz odpowiedź. Tu nie ma czasu na pytanie — odpowiedź musi być gotowa **w kilkadziesiąt nanosekund** (nanosekunda to miliardowa część sekundy).

**Shadow Ledger** (ang. "cień księgi rachunkowej") to trzymana w pamięci operacyjnej komputera kopia stanu finansowego każdego obserwowanego tokena. Działa jak ciągle aktualizowana lista cenowa.

### Jak działa wycena na Pump.fun?

Pump.fun używa automatycznego mechanizmu cenowego zwanego **bonding curve** (krzywa wiązania). Zasada jest prosta: im więcej SOL (waluta Solany) jest w puli danego tokena, tym wyższy jego kurs. I odwrotnie — każdy zakup podnosi cenę, każda sprzedaż ją obniża. To zamknięty, matematyczny system bez żadnego pośrednika.

Shadow Ledger przechowuje dla każdego tokena:
- `sol_reserves` — ile SOL jest aktualnie w puli
- `token_reserves` — ile tokenów jest w puli
- `complete` — czy token "wypełnił krzywą" (czyli dorósł do fazy migracji na Raydium, większą giełdę)

### Symulacja przed zakupem — sub-50ns

Przed jakimkolwiek zakupem Ghost wywołuje `simulate_buy()` — czystą funkcję matematyczną bez żadnego zapytania do sieci:

```
Wejście:  ile SOL chcemy wydać + akceptowalny poślizg cenowy
Wyjście:  ile tokenów dostaniemy + wpływ na cenę (%) + minimum tokenów przy slippage
```

Latencja: **poniżej 50 nanosekund**. To jest fundamentalna przewaga — zanim ktokolwiek wyśle transakcję do sieci (co zajmuje dziesiątki milisekund), Ghost już wie, czy ta transakcja ma sens ekonomiczny.

Standardowe parametry symulacji:
- Slippage (poślizg cenowy) = 500 bps = 5% — maksymalna akceptowalna różnica między ceną zakładaną a faktycznie uzyskaną
- Fee = 25 bps = 0,25% — prowizja platformy

### Spójność danych — jak Shadow Ledger się aktualizuje?

Domyślnie Shadow Ledger aktualizuje się **wyłącznie na podstawie transakcji** (`tx-only mode`). Każda zaobserwowana transakcja buy lub sell aktualizuje stan krzywej. To szybkie, ale może prowadzić do małych odchyleń od rzeczywistości. Dlatego działa **Reconciliation** (uzgadnianie) — mechanizm porównujący stan Ledgera ze stanem on-chain i klasyfikujący odchylenia:

| Typ odchylenia | Próg | Reakcja |
|----------------|------|---------|
| Szum | < 1 000 000 lamportów (~0,001 SOL) | Ignoruj |
| Znaczące | 1M – 100M lamportów | Ostrzeżenie w logach |
| Krytyczne | > 100 000 000 lamportów (~0,1 SOL) | Alert + wymuszony update |

---

## GATEKEEPER — STRAŻNIK I SERCE SYSTEMU

Gatekeeper (ang. "strażnik bramy") to absolutnie kluczowy komponent. Jego zadanie: **odfiltrować 95% tokenów, które są śmieciem, botami lub manipulacjami**, i przepuścić tylko te z realnym, organicznym ruchem.

### Dlaczego to jest trudne?

Na Pump.fun większość "akcji" przy nowym tokenie to fałszywa aktywność. Twórcy tokenów lub inne boty symulują obroty, żeby przyciągnąć prawdziwych kupujących. Gatekeeper musi odróżnić naturalny, ludzki handel od mechanicznego, zautomatyzowanego ruchu. Robi to przez analizę wzorców statystycznych zachowania — boty, choćby bardzo zaawansowane, zostawiają charakterystyczne "odciski palców" w danych.

### Jak działa buforowanie obserwacji?

Gdy pojawia się nowy token, Gatekeeper **nie reaguje natychmiast**. Otwiera **okno obserwacyjne** (domyślnie 500 ms) i gromadzi transakcje w buforze (`GatekeeperMintBuffer`). Dopiero gdy zbierze co najmniej 5 transakcji lub upłynie czas okna, uruchamia **ewaluację sześciofazową**. Pula musi zdać co najmniej 4 z 6 faz, żeby przejść dalej.

---

### FAZA 1 — Próg wejścia (Dust Filter)

**Cel:** Odfiltrowanie absolutnego śmiecia — tokenów, które nikt nie dotknął lub które były tylko testowane.

| Metryka | Próg | Co wykrywa |
|---------|------|------------|
| `min_tx_count` | ≥ 5 transakcji | Minimum aktywności |
| `min_unique_signers` | ≥ 3 unikalne portfele | Brak zainteresowania jednej osoby |
| `min_buy_count` | ≥ 3 zakupy | Realny popyt |
| `dust_threshold` | Transakcje poniżej progu pyłu są ignorowane | Odfiltrowuje "testowe" centowe transakcje |

---

### FAZA 2 — Profil prędkości (Velocity Profile)

**Cel:** Wykrycie regularności charakterystycznej dla botów. Człowiek klika nieregularnie — bot strzela transakcje jak metronom.

**Metryka 1: Coefficient of Variation czasu między transakcjami (CV)**
Wyobraź sobie, że mierzysz odstępy czasu między kolejnymi zakupami. CV to stosunek odchylenia standardowego do średniej — miara nieregularności. Bot ustawiony na "kup co 100ms" ma CV bliskie zeru. Ludzie mają CV powyżej 0,3.
- Próg: CV < 0,3 = sygnał bota | CV ≥ 0,3 = organiczne

**Metryka 2: Timing Entropy (entropia timingu)**
Shannon Entropy — miara losowości rozkładu transakcji w czasie. Czas dzielony jest na 10 równych odcinków. Jeśli wszystkie transakcje padają w jednym odcinku — entropia bliska zeru. Losowo rozłożone — wysoka entropia.
- Próg: entropia < 1,2 = bot klastry | ≥ 1,2 = organiczne

**Metryka 3: Burst Ratio (wskaźnik wybuchu)**
Jaki procent wszystkich transakcji padł w pierwszych 20% okna czasowego? Boty często bombardują na start, żeby "nakręcić" wykres.
- Próg: burst_ratio > 0,70 = podejrzane nakręcanie

**Metryka 4: Avg Interval (średni odstęp)**
Średni czas między transakcjami musi mieścić się w "ludzkim" zakresie — nie za krótki (bot), nie za długi (brak zainteresowania).
- Próg: 60 ms – 600 ms = normalne

---

### FAZA 3 — Różnorodność sygnatariuszy (Signer Diversity)

**Cel:** Wykrycie koncentracji — gdy jeden lub kilka portfeli dominuje w obrocie. To sygnał kabalu lub wash tradingu (handel ze samym sobą w celu sztucznego napompowania wolumenu).

**Metryka 5: HHI — Herfindahl-Hirschman Index**
Klasyczny wskaźnik ekonomiczny mierzący koncentrację rynku. Obliczany jako suma kwadratów udziałów rynkowych każdego uczestnika.

```
Przykład:
5 portfeli z udziałami: 20%, 20%, 20%, 20%, 20%
HHI = 0,04 + 0,04 + 0,04 + 0,04 + 0,04 = 0,20 (umiarkowane)

1 portfel z 80%, reszta po 5% (4 portfele)
HHI = 0,64 + 0,0025×4 = 0,65 (wysoka koncentracja — KABAL)
```

- Próg: HHI > 0,25 = podejrzenie kabalu | HHI > 0,5 = TWARDY FAIL (natychmiastowe odrzucenie)

**Metryka 6: Gini Coefficient (współczynnik Giniego) wolumenu**
Zapożyczony z ekonomii miernik nierówności majątkowej, tu zastosowany do wolumenu transakcji. 0,0 = wszyscy kupili tyle samo; 1,0 = jeden portfel zrobił wszystko.
- Próg: Gini > 0,70 = dominacja wielorybów

**Metryka 7: Top-3 Volume Percentage**
Jaki procent całego wolumenu wykonały trzy najbardziej aktywne portfele?
- Próg: top3 > 75% = dominacja walców | > 95% = TWARDY FAIL

**Metryka 8: Same-ms TX Ratio (wskaźnik bundlingu)**
Jaki procent transakcji przyszedł dokładnie w tej samej milisekundzie? To sygnał użycia **Jito bundlingu** — techniki wysyłania wielu transakcji w jednym pakiecie, używanej przez boty do symulowania "tłumu".
- Próg: > 30% = podejrzenie bundlingu

**Metryka 9: Max TX per Signer**
Ile transakcji maksymalnie wykonał jeden portfel?
- Próg: > 4 transakcje od jednego portfela = czerwona flaga

**Metryka 10: Unique Ratio (wskaźnik unikalności)**
Stosunek unikalnych portfeli do liczby wszystkich transakcji. Jeśli 10 transakcji zrobiło 2 portfele — unique_ratio = 0,2. Jeśli 10 transakcji zrobiło 9 portfeli — 0,9.
- Próg: < 0,4 = zbyt mało uczestników

---

### FAZA 4 — Dynamika cenowa (Price Dynamics)

**Cel:** Wykrycie manipulacji cenowej przy cienkiej płynności.

**Metryka 11: Single TX Price Impact (wpływ jednej transakcji na cenę)**
Jaki procent zmiany ceny spowodowała jedna transakcja? Jeśli jedna osoba kupiła i cena skoczyła o 15%+, oznacza to albo wieloryba manipulującego wykresem, albo ekstremalnie cienką płynność.
- Próg: > 15% = twardy fail (wieloryb-manipulator)

---

### FAZA 5 — Zachowanie developera (Dev Behavior)

**Cel:** Wykrycie "rug pull setup" — developer kupuje tanio, a potem sprzedaje wszystko na raz, rujnując kurs. Rug pull (dosł. "wyrwanie dywanu") to sytuacja, gdy twórca tokena nagle znika ze wszystkimi środkami.

**Metryka 12: Dev Buy SOL**
Ile SOL developer wrzucił przy stworzeniu tokena? Wysoki dev buy = developer ma ogromną pozycję gotową do zdumpowania.
- Próg: > 8 SOL = TWARDY FAIL

**Metryka 13: Dev TX Ratio**
Jaki procent wszystkich transakcji to transakcje developera?
- Próg: > 20% = developer dominuje obrotem

**Metryka 14: Dev Sold Within 3s / 5s**
Czy developer sprzedał swoje tokeny w ciągu 3 lub 5 sekund od stworzenia? To klasyczny sygnał "natychmiastowego runga".
- Jedna z twardzych flag = natychmiastowe odrzucenie

**Metryka 15: Dev Paperhand Latency**
Jak długo developer trzymał tokeny przed sprzedażą? Krótki czas = brak wiary twórcy w swój własny projekt.

---

### FAZA 6 — Dynamika bonding curve (Curve Dynamics)

**Cel:** Walidacja matematycznej spójności stanu puli.

**Metryka 16: Virtual Reserves Validation**
Czy deklarowane rezerwy (SOL i tokeny) są matematycznie spójne z historią transakcji? Niespójność = dane sfałszowane lub uszkodzone.

**Metryka 17: Curve Finality Check**
Czy token jest już w fazie "wypełnionej krzywej" i migruje na Raydium? Ghost nie interesuje się tokenami w tej fazie — ich wczesna szansa już minęła.

---

### Sygnały miękkie — dodatkowy system ostrzeżeń

Poza twardymi fail'ami i fazami Gatekeeper prowadzi **scoring sygnałów miękkich** — każdy podejrzany wzorzec dodaje punkty karne:

| Sygnał | Co oznacza |
|--------|-----------|
| `low_interval_cv` | Bot-like regularity — transakcje jak zegarek |
| `low_timing_entropy` | Wszystkie transakcje skupione w jednym momencie |
| `high_burst_ratio` | Bombardowanie na start (pompowanie wykresu) |
| `bundle_suspicion` | Użycie bundlingu do symulowania tłumu |
| `cabal_suspicion` | HHI zbyt wysoki — kilka portfeli kontroluje wszystko |
| `top3_dominance` | Trzy portfele = cały wolumen |
| `high_volume_gini` | Skrajna nierówność wolumenu |
| `high_tps` | Ekstremalnie szybki TPS — sygnał botów |

Wagi grup sygnałów:
- Manipulacja (kabal, bundle, whale): 30 punktów
- Timing (bot regularity): 25 punktów
- Różnorodność (Gini, unique): 20 punktów

---

### Wczesne odciski palców z PumpPortal

Seer zbiera też dodatkowe dane diagnostyczne już w momencie wykrycia nowego tokena:

| Flaga | Znaczenie |
|-------|-----------|
| `flipper_presence_ratio` | Jaki procent to "flippers" — osoby kupujące tylko po to, żeby natychmiast odsprzedać |
| `jito_tip_intensity` | Jak intensywne jest użycie Jito bundlingu |
| `whale_reversal_ratio_top3` | Czy top 3 portfele sprzedały zaraz po zakupie |
| `early_slot_volume_dominance_buy` | Czy wolumen skupił się ekstremalnie wcześnie |

---

### Wynik Gatekeepera

Pula musi zdać **minimum 4 z 6 faz** i nie trafić w żaden **twardy fail**. Jeśli przejdzie — trafia do dalszego pipeline. Jeśli nie — jest usuwana i zapominana.

---

## ORACLE — MÓZG DECYZYJNY

Po przejściu Gatekeepera pula trafia do silnika oceniającego. Oracle analizuje kandydata i daje odpowiedź binarną: wchodzę czy nie.

Silnik działa w **12 cyklach (S1–S12)** po około 420 ms każdy — łącznie około 5 sekund analizy. Podział cykli jest nieprzypadkowy:
- **Cykle S1–S2:** tryb snajperski — agresywne wczesne wykrywanie okazji. System może podjąć decyzję już tu, jeśli sygnały są ekstremalnie silne.
- **Cykle S3–S7:** stabilizacja — budowanie pewności oceny.
- **Cykle S8–S12:** finalny werdykt — ważone podjęcie decyzji.

Mechanizm **"gunshot"** (strzał) pozwala na wczesne zakończenie analizy, jeśli któryś wskaźnik przekroczy próg pewności przed końcem 12 cykli. Nie marnuj czasu na pewniaki.

### Równolegle działające moduły oceny

Oracle Pipeline uruchamia **7 równoległych workerów** jednocześnie:

**1. HyperPredictionOracle** — główny silnik oceniający, integrujący sygnały z pozostałych modułów, produkuje finalny wynik 0–100.

**2. HyperOracle** — analizuje koherencję sygnałów (czy dane z różnych źródeł "zgadzają się" ze sobą).

**3. ClusterHunter (Łowca Kabali)** — specjalistyczny moduł do wykrywania skoordynowanych grup portfeli działających razem. Analizuje sieć powiązań między portfelami, historię ich współpracy, wzorce synchronizacji transakcji.

**4. DevProfiler (Profiler Developera)** — analizuje historię portfela twórcy tokena: ile poprzednich tokenów stworzył, ile z nich skończyło rugiem, ile ma na swoim koncie "paper hands" (szybka sprzedaż po starcie).

**5. VisionCritic** — analizuje jakość materiałów marketingowych tokena (logo, opis, metadane). Śmieciowe tokeny mają śmieciowe "opakowanie".

**6. IWIM (Insider Wallet Influence Map — Mapa Wpływu Portfeli Insiderów)** — mapuje powiązania między portfelami i identyfikuje wzorce insider tradingu (handlu z wykorzystaniem informacji poufnych), sybil attacks (jedna osoba, wiele portfeli udających niezależnych inwestorów) i scam patterns.

**7. BvaClassification (Klasyfikator ruchu)** — klasyfikuje typ aktywności w puli:
- `Organic` (organiczne) — naturalni inwestorzy, prawdopodobieństwo a priori 85%
- `Chaotic` (chaotyczne) — wysoka zmienność bez wzorca
- `Dormant` (uśpione) — brak aktywności
- `Steered` (sterowane) — widoczna ręka manipulatora, tylko 15% szans na sukces

Wyniki wszystkich 7 modułów są agregowane w `combined_score` (0–100). **Próg przejścia: 70**.

---

## TRIGGER — RĘKA WYKONAWCZA

Gdy Oracle daje "zielone światło", Trigger (`ghost-launcher/src/components/trigger/`) buduje i wysyła transakcję zakupu.

### Symulacja przed wysłaniem

Nawet po pozytywnym wyniku Oracle, Trigger uruchamia jeszcze jedną weryfikację — **shadow simulation** (symulacja cienia): wysyła transakcję do sieci w trybie "tylko symuluj, nie wykonuj" i sprawdza, czy wszystko jest OK zanim wyda prawdziwe pieniądze.

### Obliczanie rozmiaru pozycji

Ile kupujemy? Zależy od pewności systemu:

```
Baza: max_position_size_sol = 0,0001 SOL (konfigurowalny)

Korekta za poziom ryzyka:
  Niskie ryzyko    → 100% bazy
  Średnie ryzyko   → 75% bazy
  Wysokie ryzyko   → 50% bazy
  Bardzo wysokie   → 25% bazy

Korekta za pewność (gdy score < 80):
  rozmiar = rozmiar × (score / 100)

Absolutne minimum: 1 000 lamportów
  (lamport = 0,000000001 SOL — najmniejsza jednostka Solany)
```

### Jito Tips — opłata za pierwszeństwo

Na Solanie możesz "napiwkować" walidatora (uczestnika sieci potwierdzającego transakcje), żeby umieścił Twoją transakcję na początku bloku. Ghost używa dynamicznych napiwków Jito:
- Baza: 0,01% wartości transakcji
- Dynamika: do 2%
- Absolutny maksimum: 0,0001 SOL lub 0,4% wartości (ochrona przed przebiciem)

System **nigdy nie wejdzie w licytację napiwków** z innymi botami — górny limit jest twardy.

### Tryb shadow-only (aktualny stan)

Obecna konfiguracja: `entry_mode = "shadow_only"`. Ghost buduje transakcje, symuluje je, śledzi wyniki — ale ich **nie wysyła**. To faza kalibracji: system uczy się na realnych danych, zanim ryzyknie prawdziwy kapitał.

---

## POST-BUY — CO SIĘ DZIEJE PO ZAKUPIE

### PostBuyRuntime — cienka warstwa koordynacji

Gdy zakup zostaje złożony (lub zasymulowany), do akcji wkracza `PostBuyRuntime`. Jest to celowo uproszczony komponent — jego jedynym zadaniem jest **mapowanie zdarzeń z Event Busu na logikę zarządzania pozycją**. Cała prawdziwa inteligencja zarządzania pozycją mieszka w `ghost-brain` (mózg systemu), konkretnie w `PaperPositionLifecycle`.

### PaperPositionLifecycle — pełny cykl życia pozycji

Każda otwarta pozycja przechodzi przez 5 etapów:

**1. Faza wejścia (Entry Phase)**
- Zlecenie zakupu jest emitowane i monitorowane
- `PaperBroker` odpytuje co 200–400 ms, czy zakup został "wypełniony" (potwierdzony)
- Timeout wejścia: 5 sekund

**2. Otwarcie pozycji (Position Open)**
- Pozycja jest rejestrowana w systemie zarządzania
- Startuje pętla taktyczna (tick loop)

**3. Pętla taktyczna (Tick Loop, co 500 ms)**
- `AemRuntime` — Adaptive Execution Manager (Adaptacyjny Menedżer Egzekucji) — odpytuje system o decyzję
- AEM może zlecić: trzymaj dalej, zrealizuj zysk, wytnij stratę
- Horyzont decyzji AEM: **120 sekund** od otwarcia
- Maksymalna liczba "ticków" przed wymuszonym wyjściem: **240 ticków** (= 120 sekund)

**4. Wyjście (Exit Phase)**
- Zlecenie sprzedaży
- Monitorowanie potwierdzenia wypełnienia
- Zapis wyniku

**5. Zamknięcie (Finalization)**
- Emisja eventu `PositionClosed`
- Zapis do logu JSONL (format używany do analizy wyników)

---

## REVOLVER — AUTOMATYCZNY MECHANIZM WYJŚCIA

**Revolver** to jeden z najbardziej eleganckich komponentów systemu. Jego idea: zamiast w momencie decyzji o sprzedaży budować i wysyłać transakcję (co zajmuje czas), **przygotuj transakcje sprzedaży z góry** i trzymaj je "naładowane" jak magazynek rewolweru. Gdy warunek jest spełniony — "strzelasz" gotową transakcją.

### Bullets — naboje

Każdy "nabój" (`Bullet`) to kompletna, podpisana kryptograficznie transakcja sprzedaży z następującymi parametrami:

| Parametr | Opis |
|----------|------|
| `target_price` | Cena wyzwalająca sprzedaż (w lamportach) |
| `position_fraction_bps` | Jaki procent pozycji sprzedać (2500 bps = 25%) |
| `tx_bytes` | Gotowa, podpisana transakcja (bajty do wysłania) |
| `time_stop_secs` | Wymuszony strzał po N sekundach niezależnie od ceny |

### Domyślne poziomy realizacji zysku (Take-Profit)

```
TP1: cena +25%  → sprzedaj 25% pozycji
TP2: cena +50%  → sprzedaj kolejne 25% pozycji
TP3: cena +100% → sprzedaj pozostałe 50% pozycji

Time Stop: po 20 minutach sprzedaj wszystko niezależnie od ceny
```

Ta strategia realizuje zyski stopniowo — nie wszystko naraz. Zabezpiecza część zysku wcześnie, jednocześnie pozwalając reszcie pozycji "biec" przy silnym trendzie wzrostowym.

### Revolver Worker — strażnik świeżości nabojów

Na blockchainie Solana każda transakcja zawiera **recent blockhash** — unikalny identyfikator ostatniego bloku, który "wygasa" po około 60–90 sekundach. Jeśli transakcja ma stary blockhash, zostanie odrzucona przez sieć.

`Revolver Worker` działa w tle i co **30 sekund** sprawdza, które naboje mają zbliżający się termin "ważności" (starsze niż 60 sekund). Dla takich nabojów:
1. Pobiera świeży blockhash z sieci
2. Ponownie podpisuje transakcję
3. Podmienia stary nabój na świeży

Cały mechanizm używa sprytnej strategii blokowania: **read lock** (blokada do odczytu, współdzielona) do identyfikacji przeterminowanych nabojów, **write lock** (blokada wyłączna) tylko do samej podmiany. Minimalizuje to czas blokowania gorącej ścieżki.

### Shot Event Tracking — śledzenie strzałów

Każdy "strzał" (wysłana transakcja sprzedaży) jest śledzony przez dwa etapy:
- `Submitted` — transakcja wysłana do sieci (TPU — Transaction Processing Unit, węzeł sieci przyjmujący transakcje)
- `Filled` — transakcja potwierdzona w bloku

Każde zdarzenie zawiera: ID zlecenia, adres tokena, ID kandydata, ID pozycji, cenę wyzwalającą, procent sprzedanej pozycji, faktyczną cenę zaobserwowaną, sygnaturę transakcji.

---

## TRWAŁOŚĆ DANYCH — CO SIĘ DZIEJE PRZY CRASHU

Systemy finansowe muszą przeżyć awarie bez utraty stanu. Ghost ma dwa mechanizmy ochrony danych (zrealizowane w ramach kamieni milowych Z1.1 i Z1.2):

### WAL — Write-Ahead Log (Dziennik przed-zapisowy)

Każda zmiana stanu systemu jest **najpierw zapisywana do pliku dziennika**, a dopiero potem wprowadzana. Jeśli system crashuje w trakcie operacji — dziennik pozwala odtworzyć stan do momentu awarii.

Dziennik działa w segmentach (plikach) rotowanych co 5 minut. Stare segmenty są usuwane po 24 godzinach. Domyślny tryb synchronizacji: `Sync` — po każdym wpisie system wykonuje `fsync` (gwarantuje, że dane fizycznie trafiły na dysk, nie zostały w buforze systemu operacyjnego).

Typy zapisywanych rekordów:
- Surowe transakcje (do recovery)
- Sparsowane zdarzenia
- Decyzje systemu (Kup / Odrzuć)
- Zlecenia handlowe
- Aktualizacje Shadow Ledger

### Disk Snapshots — pełne zrzuty stanu

Co 60 sekund system zapisuje **pełny stan Shadow Ledger** na dysk (`data/snapshots/`). Format binarny z wersjonowaniem.

Po restarcie:
1. Wczytaj ostatni snapshot (stan sprzed maksymalnie ~60 sekund)
2. Replay WAL od momentu snapshotu do teraz
3. Stan odtworzony — system gotowy do pracy

Kluczowy szczegół: replay WAL jest **deterministyczny** — transakcje są sortowane według sygnatury (unikalnego identyfikatora z blockchainu), nie według zegara systemowego. To gwarantuje, że niezależnie od tego, kiedy i jak crashnął system, po restarcie odtworzymy dokładnie ten sam stan.

---

## POKRYCIE TESTAMI — PRAWIE PEŁNE (~99%)

### Filozofia: bez mocków na warstwie storage

Projekt ma jedno żelazne podejście do testów: **integracyjne testy używają prawdziwych komponentów, nie imitacji (mocków)**. To zasada wymuszona przez przeszłe doświadczenie, gdy "zamockowane" testy przechodziły zielono, ale prawdziwa migracja na produkcji failowała.

### Kluczowe zestawy testów

**Gatekeeper V2 Pipeline Integration** — test E2E filtra:
- Scenariusz "Kup": 5 organicznych transakcji z różnych portfeli, zróżnicowane kwoty, chaotyczny timing → oczekiwany wynik: fazy zdane ≥ 4, pula trafia do dalszego pipeline
- Scenariusz "Odrzuć": 6 identycznych transakcji z jednego portfela, regularny timing → oczekiwany wynik: fail fazy 3 (HHI za wysoki), timeout, pula usunięta

**Snapshot Engine Data Reliability** — niezawodność historii cenowej:
- Rozróżnienie danych "miękkich" (WebSocket) i "twardych" (on-chain)
- Walidacja cross-source z tolerancją 10%
- Resync co 5 bloków
- Detekcja duplikatów

**WAL Startup Recovery** — recovery po crashu:
- Zapis sekwencji rekordów
- Symulacja awarii
- Replay i weryfikacja odtworzonego stanu

**Oracle Continuous Sampling** — ciągłe próbkowanie:
- 6 cykli po 8,4 sekundy każdy
- Walidacja stabilności wyników między cyklami

**Scenariusz A (single pool):** target: land rate ≥ 95%, inclusion rate ≥ 92%
**Scenariusz B (burst 10 pul / 30s):** te same targety + latencja Oracle < 500ms, Trigger < 200ms

---

## GDZIE JESTEŚMY TERAZ — AKTUALNY STAN PROJEKTU

System jest **produkcyjnie gotowy** pod względem infrastruktury (WAL, snapshots, Gatekeeper V2, Oracle z 7 workerami, Revolver). Działa w trybie `shadow_only` — obserwuje, analizuje, symuluje, loguje wyniki — ale nie ryzykuje prawdziwego kapitału.

Trwają prace audytowe (Faza 0 — "zamrożenie kontraktów i audyt blast radius"), których wynikiem są dokumenty ADR (Architecture Decision Records — oficjalne rekordy decyzji architektonicznych) 0010–0015. Celem audytu jest weryfikacja, że wszystkie ścieżki recovery są poprawne i bezpieczne, zanim zostanie włączony live trading.

---

## KLUCZOWE LICZBY W JEDNYM MIEJSCU

| Parametr | Wartość | Co oznacza |
|----------|---------|-----------|
| Bufor Event Bus | 10 240 eventów | ~5–10s buforu przy szczycie |
| Okno obserwacji Gatekeepera | 500 ms | Czas zbierania danych przed decyzją |
| Minimum transakcji do oceny | 5 | Mniej = nie ma co analizować |
| Fazy wymagane do przejścia | 4 z 6 | Wymagania Gatekeepera |
| HHI max (kabal) | 0,25 | Powyżej = podejrzenie kabalu |
| HHI twardy fail | 0,50 | Powyżej = natychmiastowe odrzucenie |
| Gini max (wieloryby) | 0,70 | Powyżej = dominacja jednej strony |
| Top-3 volume max | 75% | Powyżej = dominacja walców |
| Dev buy max | 8 SOL | Powyżej = ryzyko runga |
| Próg Oracle | ≥ 70/100 | Poniżej = odrzucone |
| Latencja symulacji pre-trade | < 50 ns | Zanim cokolwiek wyślemy |
| Slippage | 5% (500 bps) | Akceptowalny poślizg cenowy |
| TP1 | +25% ceny | Sprzedaj 25% pozycji |
| TP2 | +50% ceny | Sprzedaj kolejne 25% |
| TP3 | +100% ceny | Sprzedaj pozostałe 50% |
| Time stop | 20 minut | Wyjście niezależnie od ceny |
| Revolver refresh | co 30 sekund | Odświeżanie podpisów transakcji |
| WAL segment | 5 minut | Rotacja pliku dziennika |
| Snapshot interval | 60 sekund | Pełny zrzut stanu |
| WAL retencja | 24 godziny | Czas przechowywania dziennika |

---

*Recenzja oparta na kodzie HEAD `567bc60`, stan na 21.03.2026. System działa w trybie shadow-only — faza przedprodukcyjna.*
