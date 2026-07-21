# ADR-8D: HET-PM V2 jako aktywny manager sprzedaży shadow

Status: `IMPLEMENTED / SHADOW ONLY`

Typ: ADR-8D / post-buy manager / runtime

Data: `2026-07-19`

Repozytorium: `/root/Gho_dynamic_exit_v1_pr2b`

Uwaga o szablonie: wskazany w globalnych instrukcjach plik
`/root/Gho/docs/ADR/ADR_8D_SZABLON.md` nie istnieje. Dokument używa lokalnego
układu D1--D8 stosowanego przez pozostałe ADR-8D tego repozytorium.

## D1. Problem

Dotychczas HET-PM V2 liczył decyzje i zapisywał je do sidecara, ale żadna z
nich nie zamykała nawet pozycji shadow. Faktycznym managerem pozostawał V1.
To nie realizowało celu Position Managera V2.

## D2. Decyzja

Profil `ghost_brain_het_pm_v2_promotion_evidence_v1.toml` uruchamia teraz:

```toml
[post_buy_guardian.het_pm_v2]
enabled = true
mode = "authoritative_shadow"
```

W tym trybie V2 wybiera nową decyzję sprzedaży dla pozycji shadow. Istniejący
kod V1 wykonuje wyłącznie mechanikę wspólną dla już wybranej decyzji: pełny
quote, proposal, retry, symulowane wypełnienie, terminal i zwolnienie slotu.
Nie wybiera nowego TP, SL ani inactivity exitu.

Nie włączono żadnego live sell ani live execution. Launcher odrzuca ten tryb,
jeżeli runtime nie ma jednocześnie `execution_mode = shadow` i
`entry_mode = shadow_only`.

## D3. Reguły działania

1. V2 wybiera spośród: Crash, HardLoss, ExecutableTrailing, VitalityDecay i
   AbsoluteMaxHold.
2. Crash działa tylko wtedy, gdy jego własny tryb V1 jest
   `authoritative_shadow`. Przy `observe_only` jest zapisany diagnostycznie,
   ale jest pomijany przy wyborze sprzedaży i nie blokuje niższego Trailingu.
3. Gdy przed uruchomieniem V2 istnieje rozpoczęty proposal V1, jest on
   dokańczany z pierwotnym action ID i reason. V2 nie podmienia go w locie.
4. AbsoluteMaxHold ma pierwszeństwo nad brakującą trasą, markiem, trajektorią
   albo vitality data. Runtime podejmie próbę pełnej sprzedaży; jeżeli quote
   nie może być rozwiązany, pozostanie istniejąca typed ścieżka recovery,
   zamiast bezterminowego otwarcia pozycji.

## D4. Stany wykluczone

- brak równoległego V1 i V2 jako autorów nowej sprzedaży;
- brak automatycznego podniesienia CrashGuard z `observe_only`;
- brak modyfikacji buy/reject Gatekeepera;
- brak live submitu, live sellu lub zmiany konfiguracji live;
- brak zapisu sidecara jako warunku powodzenia terminalu.

## D5. Zapisy śladu

Comparison row rozróżnia decyzję wybraną przez V2 od starego action V1
dokańczanego po przełączeniu. Gdy V1 action został rozpoczęty wcześniej,
`v2_proposal_created` i `v2_economic_mutation` pozostają `false`; nie
przypisują V2 cudzej sprzedaży.

Wiersz startowy runtime podaje także faktyczny stan: przy tym profilu musi
zawierać `decision_owner=v2`, `v2_shadow_authority=true`,
`v1_shadow_authority=false` i `live_authority=false`. Dzięki temu log nie
może opisywać aktywnego managera jako obserwatora V2.

## D6. Testy wykonane przy zmianie

1. Jedna realna próbka zawierająca Crash i Trailing: Crash w `observe_only`
   jest pomijany, a Trailing zostaje wybrany.
2. Pozycja na 10x ceny wejścia nie jest zamykana przez stare V1 TP; po
   `absolute_max_hold_ms` V2 wykonuje pełny exit shadow z reason
   `absolute_max_hold`, mimo blokera danych trasy/trajektorii.
3. Preexisting V1 take-profit proposal zostaje wykonany z tym samym action ID
   i reason, mimo że bieżący V2 tick osiągnąłby własny MaxHold.
4. Launcher akceptuje tryb V2 wyłącznie w kompletnym runtime shadow i odrzuca
   go w paper/live.
5. Status runtime dla `authoritative_shadow` jest testowany jako V2 owner,
   V1 nie-owner oraz brak live authority.

## D6a. Profil uruchomieniowy

Do realnego runu shadow progi managera są celowo łagodne: Trailing uzbraja
się od 5% wzrostu, wymaga 3% cofnięcia marku i 1% cofnięcia quote; Vitality
może zadziałać od 5 sekund i jednego słabego okna. Nie zmieniono minimów
wejścia Gatekeepera: 5 transakcji, 3 kupna, 3 unikalnych signerów.

## D6b. Stabilność procesu runu shadow

Pierwsze uruchomienie aktywnego managera zakończyło się przed otwarciem
jakiejkolwiek pozycji przez `tokio-rt-worker stack overflow`. Nie był to exit
V2 ani wynik wejścia Gatekeepera: writer HET zapisał zero comparison rows,
a lifecycle shadow nie zawierał otwartej pozycji.

Launcher buduje więc jawnie własny wielowątkowy runtime Tokio ze stosem
workera 16 MiB. Jest to lokalna ochrona procesu dla głębokiego łańcucha
protobuf → account state → sesja, bez zmiany logiki wejścia, decyzji V2,
symulowanej sprzedaży albo ścieżki live. Następny run shadow ma potwierdzić,
że proces pozostaje aktywny wystarczająco długo, by manager wykonał faktyczną
decyzję na pozycji.

## D6c. Payer symulacji wejścia

Pełny run retry1 nie zakończył się awarią, ale ujawnił drugi błąd
konfiguracji: losowy payer `ephemeral` nie istnieje w łańcuchu. Wszystkie
338 prób `simulateTransaction` zakończyły się `AccountNotFound`; żadna nie
stała się aktywną pozycją shadow, więc manager nie otrzymał ani jednego ticku.

Profil retry2 używa zatem istniejącego, zasilonego klucza wyłącznie jako
payera symulacji. Nadal ma `entry_mode = "shadow_only"` i
`execution_mode = "shadow"`. W tej kombinacji kod wywołuje wyłącznie
`simulate_buy`; gałąź submitu live jest niedostępna. Celem poprawki jest
utworzenie rzeczywistych pozycji shadow, które V2 może zamykać, a nie zmiana
jakiejkolwiek ścieżki live.

## D6d. Dokładne przypisanie decyzji V2 do sprzedaży

Comparison row zachowuje teraz dokładny reason, który V2 przekazał do
wspólnego wykonawcy shadow. To pole jest odrębne od diagnostycznego zwycięzcy
sztywnej hierarchii: wyższa bramka może być zablokowana, a niezależny twardy
limit `AbsoluteMaxHold` może być rzeczywistą decyzją sprzedaży. Finalizacja
oznacza więc proposal i fill jako V2 wyłącznie wtedy, gdy receipt odpowiada
tej zapisanej decyzji V2. Usuwa to również rozjazd między V2 `HardLoss` a
istniejącym reason wykonawcy `stop_loss`, bez przypisania V2 starego proposal
V1 dokańczanego po przełączeniu.

## D6e. Świeży stan krzywej po ucichnięciu poola

Analiza retry3 wykazała, że problem nie był utratą aktualizacji pomiędzy
Seerem a `AccountStateCore`: każda pozycja, dla której po wejściu wystąpił
trade, miała również późniejszy `AccountUpdate`. Problemem był pool, który po
wejściu przestawał emitować ruch. W takim przypadku ostatni stan krzywej
uczciwie stawał się stary, a V2 nie może na nim tworzyć quote'u ani symulować
sprzedaży.

Dodano więc `post_buy_guardian.shadow_market_refresh`. Tylko dla już
otwartych pozycji **shadow**, których kanoniczny stan krzywej przekroczył
próg świeżości, osobny task wykonuje bounded `getAccountInfo` z commitmentem
`processed`. Dekoduje faktyczne rezerwy bonding curve i publikuje je do
`AccountStateCore` z oznaczeniem `RpcRefresh`. Ten task ma timeout per read,
limit równoległości, cooldown per pozycja i round-robin, dlatego nie blokuje
ticku managera, terminal commit ani zwolnienia capacity.

To nie jest retimestamping starej ceny. Gdy RPC nie odpowie, konto nie
istnieje albo dekodowanie się nie powiedzie, nie powstaje żadna nowa cena i
V2 pozostaje fail-closed. Mechanizm nie buduje, nie wysyła i nie potwierdza
żadnej transakcji; ścieżka live pozostaje wyłączona.

## D6f. Preflight kompletnego managera

Pierwsza próba uruchomienia odświeżacza wykryła błąd samego profilu: tabela
`shadow_market_refresh` została wpisana przed trzema bezpośrednimi polami
`post_buy_guardian`. Zgodnie z semantyką TOML pola V1 zostały więc przypisane
do tej tabeli i runtime słusznie odrzucił brak `target_threshold` przed
subskrypcją.

Tabela odświeżacza znajduje się teraz po wszystkich bezpośrednich polach
`post_buy_guardian`. Ponadto `ghost-launcher --preflight` ładuje pełny brain
config i waliduje V1 oraz HET-PM V2 dokładnie przed uruchomieniem. Błąd
brakującego TP/SL, nieaktywny guardian lub niepoprawna konfiguracja V2 nie
może już minąć preflightu i zatrzymać runu dopiero po starcie komponentów.

## D7. Uruchomienie

Następny run używa profilu shadow z tym brain configiem. Analiza następuje po
pojawieniu się faktycznych wejść i wyjść w logach shadow: sprawdzane będą
reason sprzedaży, action ID, pełna ilość oraz zwolnienie pozycji. Nie tworzy
się dodatkowego trybu observe-only zamiast tej weryfikacji.

## D8. Rollback

Rollback to jedna zmiana konfiguracji:

```toml
mode = "observe_only"
```

Nie wymaga migracji pozycji, nie dotyka live execution i pozostawia aktywne
proposals dokończone przez ich istniejącą ścieżkę V1.
