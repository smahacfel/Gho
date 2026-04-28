# PLAN WYKONAWCZY SELL — etapowe domknięcie logiki post-buy do produkcyjnego live exit

## 1. Cel

Celem tego planu jest doprowadzenie logiki biznesowej **post-buy** do stanu kompletnego, produkcyjnego procesu live exit, zgodnego z:

- obecną architekturą po refaktorze,
- obowiązującym SSOT,
- zaakceptowanymi ADR-ami,
- istniejącymi kontraktami runtime i event bus.

Plan ma być rozwijany **sekwencyjnie**. Rozwijamy kolejny etap dopiero po zamknięciu poprzedniego. Na ten moment plan uszczegóławia wyłącznie **etap startowy**, którego celem jest domknięcie zmiany architektury i dostarczenie pierwszej, uproszczonej, ale produkcyjnie wiarygodnej logiki wyjścia.

---

## 2. Stan wyjściowy i twarde ograniczenia

### 2.1. Obowiązujące SSOT / granice architektury

1. **Live SELL należy do `ghost-launcher`**, nie do `ghost-brain`.
   - SSOT live exit: `ghost-launcher/src/components/post_buy_runtime.rs`
   - `MonitoringEngine` / `Guardian` / paper lifecycle nie są live path.

2. **Transport live BUY i live SELL ma pozostać fail-closed na Jito Bundle over gRPC**.
   - brak fallbacku do RPC submit,
   - brak degradacji live -> paper.

3. **`AccountStateCore` jest canonical-first źródłem live price / live market truth**.
   - `ShadowLedger` może pozostać wyłącznie compare-only / diagnostyczny,
   - RPC point query może zostać jedynie bounded fallbackiem odczytowym, nie SSOT logiki biznesowej.

4. **Zmiana architektury nie może przywrócić starego ownership splitu**.
   - live pozycja po potwierdzonym BUY ma jednego właściciela,
   - właścicielem lifecycle exit pozostaje launcherowy runtime post-buy.

### 2.2. Aktualny problem do rozwiązania w pierwszym etapie

Obecny live path ma już:

- potwierdzony BUY przez Jito gRPC,
- live SELL submit przez Jito gRPC,
- canonical-first odczyt ceny z `AccountStateCore`,
- fail-closed brak paper fallbacku.

Ale nadal nie daje jeszcze prostego, jednoznacznego, produkcyjnego proof-lane dla refaktoru, bo:

1. operuje na **strategii TP ladder + bullets + time-stop**, a nie na prostym full-exit,
2. nie traktuje **realnej ceny zakupu** jako osobnego, utrwalanego kontraktu biznesowego,
3. ma kilka gałęzi `no-sell` przed pierwszą realną próbą wyjścia,
4. nie zamyka jeszcze w sposób twardy ownership/handoff/terminalizacji pozycji,
5. nie daje maksymalnie prostego i audytowalnego dowodu:  
   **BUY landed -> entry price persisted -> price monitored -> +30%/-30% -> SELL 100% submitted and confirmed**.

---

## 3. Zasada prowadzenia planu

Ten plan jest realizowany etapami:

1. **Najpierw domykamy architekturę etapu 1.**
2. **Dopiero potem implementujemy uproszczony mechanizm wyjścia.**
3. **Dopiero po lokalnym i runtime proofie etapu 1 rozwijamy kolejne elementy logiki SELL.**

Na obecnym etapie nie planujemy jeszcze:

- złożonych TP ladderów,
- trailing stopów,
- time-stop jako podstawowej semantyki wyjścia,
- częściowych sprzedaży,
- rozbudowanej strategii zarządzania pozycją.

To wszystko jest **poza zakresem etapu 1**.

---

## 4. ETAP 1 — domknięcie architektury pod uproszczony live exit

## 4.1. Cel etapu

Dostarczyć **minimalny, produkcyjnie wiarygodny proof-lane** dla live post-buy:

- po potwierdzonym BUY zapisujemy prawdziwą cenę wejścia,
- monitorujemy cenę tokena na bieżąco,
- liczymy profit / loss względem ceny zakupu,
- przy **+30%** wykonujemy **SELL 100%**,
- przy **-30%** wykonujemy **SELL 100%**,
- wszystko odbywa się w zgodzie z aktualną architekturą i bez ukrytych fallbacków semantycznych.

## 4.2. Decyzje architektoniczne, które trzeba zamrozić na samym początku

### A. Zamrozić osobny proof-lane dla uproszczonego SELL

Pierwszy etap nie powinien adaptować obecnej strategii:

- `SellStrategyConfig::default()`
- 3 TP levels,
- partial exits,
- time-stop jako głównego mechanizmu zamykania pozycji.

**Decyzja:** dla etapu 1 wprowadzamy **dedykowany uproszczony live-exit lane**, którego semantyka jest osobna od aktualnego multi-bullet TP ladder.

Powód:

- obecny bullet model wnosi zbyt dużo semantyki niepotrzebnej do udowodnienia refaktoru,
- utrudnia audit-proof,
- zwiększa liczbę gałęzi `no-sell`,
- rozmywa odpowiedź na pytanie, czy nowa architektura rzeczywiście domyka pełny BUY -> SELL lifecycle.

### B. Zamrozić pojedynczy ownership model pozycji

Po `LiveConfirmed BUY` musi powstać **jeden, jawny owner exit lifecycle**:

- launcherowy post-buy runtime,
- bez delegacji do paper runtime,
- bez utraty `position_slot_id`,
- bez auto-release przed terminalnym zakończeniem pozycji.

To oznacza, że domknięcie etapu 1 obejmuje również uszczelnienie handoffu:

- `ActivePositionLease`,
- `PostBuySubmitted`,
- direct handoff + broadcast fallback,
- terminal release slotu wyłącznie po zakończeniu sesji exit.

### C. Zamrozić SSOT danych biznesowych

W etapie 1 obowiązuje rozdział:

| Dane | SSOT |
|---|---|
| cena wejścia | potwierdzone metadata realnego BUY tx |
| stan pozycji post-buy | launcherowy live exit session / state machine |
| bieżąca cena | `AccountStateCore` |
| compare-only cena pomocnicza | `ShadowLedger` |
| read-only fallback ceny | RPC point query |
| submit/confirm SELL | Jito gRPC bundle |

### D. Zamrozić semantykę pierwszego exit triggera

Etap 1 ma tylko dwa warunki biznesowe:

1. `current_price >= entry_price * 1.30` -> **sell all**
2. `current_price <= entry_price * 0.70` -> **sell all**

Brak innych triggerów biznesowych w proof-lane.

---

## 4.3. Docelowy model etapu 1

### 4.3.1. Wprowadzany byt domenowy

Należy wprowadzić jawny rekord/sesję live exit dla pojedynczej pozycji, zawierający minimum:

- `candidate_id`
- `pool_amm_id`
- `base_mint`
- `buy_signature`
- `buy_landed_slot`
- `position_slot_id`
- `tokens_received`
- `sol_spent_lamports`
- `entry_price_lamports_per_token`
- `upper_exit_price_lamports_per_token`
- `lower_exit_price_lamports_per_token`
- `latest_price_lamports_per_token`
- `latest_pnl_pct`
- `status`
- `exit_signature` / `exit_bundle_uuid` / `exit_landed_slot`
- `terminal_reason`

Ten byt ma być **jedynym operacyjnym źródłem prawdy o post-buy lifecycle** dla etapu 1.

### 4.3.2. Minimalna state machine etapu 1

Docelowa minimalna maszyna stanów:

1. `BuyConfirmed`
2. `EntryPricePending`
3. `Armed`
4. `Monitoring`
5. `ExitTriggeredTakeProfit`
6. `ExitTriggeredStopLoss`
7. `ExitSubmitted`
8. `ExitConfirmed`

Jawne stany fail-closed:

1. `EntryPriceFailed`
2. `MonitoringUnavailable`
3. `ExitBuildFailed`
4. `ExitSubmitFailed`
5. `ExitConfirmFailed`
6. `LifecycleAbortedWithReason`

Każdy z nich musi mieć **twardy powód terminalny**, bez cichych returnów udających sukces.

---

## 4.4. Zakres prac etapu 1

### 4.4.1. Utrwalenie prawdziwej ceny wejścia jako kontraktu biznesowego

To jest pierwszy obowiązkowy element domknięcia architektury.

#### Założenie

Cena wejścia **nie może być tylko pochodną**:

- `amount_sol`,
- `actual ATA balance`,
- przybliżeń wynikających z późniejszej obserwacji konta.

#### Decyzja

Źródłem ceny wejścia ma być **potwierdzony BUY transaction metadata** odczytany po `LiveConfirmed`, po:

- `buy_signature`,
- wallet payer,
- `base_mint`.

Repo ma już prior art:

- `off-chain/components/trigger/src/transaction_monitor.rs`
- `off-chain/components/trigger/src/entry_price_extractor.rs`

Etap 1 musi wykorzystać ten kierunek zamiast dalej traktować `amount_lamports / actual_tokens` jako główną semantykę biznesową.

#### Wynik etapu

Po potwierdzonym BUY runtime ma znać i zapisać:

- ile realnie wydano SOL,
- ile realnie otrzymano tokenów,
- po jakiej realnej cenie wejścia otwarto pozycję,
- w którym slocie BUY landed.

### 4.4.2. Przebudowa live exit z TP ladder na prosty full-exit path

Dla etapu 1 należy wprowadzić osobną ścieżkę wykonawczą:

- bez 3 TP bullets,
- bez partial sells,
- bez domyślnego time-stop jako podstawowej semantyki,
- bez zależności od `SellStrategyConfig::default()` jako kontraktu proof-lane.

#### Wymagana semantyka

Pozycja ma tylko jeden aktywny zamiar wyjścia:

- **SELL 100% po +30%**
albo
- **SELL 100% po -30%**

W praktyce oznacza to, że dla etapu 1 należy preferować:

- pojedynczy full-exit builder / one-shot sell request,
albo
- wyraźnie odseparowany uproszczony path nad obecnym builderem,

zamiast nadpisywania istniejącej semantyki wielopoziomowego magazynka.

### 4.4.3. Uporządkowanie ownership i handoffu po BUY

To jest część etapu 1, nie osobny późniejszy cleanup.

Do zamknięcia:

1. `ActivePositionLease` nie może zostać zwolniony przy przejściu przez oracle handoff.
2. `PostBuySubmitted` nie może prowadzić do stanu `bought=true`, jeśli lifecycle exit nie ma ownera lub handoff nie został skutecznie ustanowiony.
3. direct handoff ma być traktowany jako ścieżka preferowana, a utrata eventu na broadcast nie może zostawiać pozycji bez jawnego terminalnego statusu.
4. `position_slot_id` ma być zwalniany wyłącznie po terminalnym końcu lifecycle.

### 4.4.4. Canonical monitoring ceny i PnL

Monitorowanie ceny ma pozostać zgodne z obecną architekturą:

1. primary source: `AccountStateCore`
2. bounded fallback read-only: RPC point query
3. compare-only: `ShadowLedger`

#### Warunek biznesowy

PnL / price change ma być liczony:

- od zapisanej realnej ceny wejścia,
- przy każdym nowym materialnym update ceny dla obserwowanego tokena,
- w szczególności na podstawie flow zdarzeń, które aktualizują canonical state dla kupionego poola/tokena.

Etap 1 musi jasno rozdzielić:

- **źródło prawdy dla triggera**,
- **źródła pomocnicze do diagnostyki**.

`ShadowLedger` nie może aktywować wyjścia.

### 4.4.5. Twarde reguły wyjścia

Etap 1 wdraża wyłącznie:

1. **take-profit +30%**
2. **stop-loss -30%**

Po ich trafieniu runtime:

- buduje pełny SELL 100%,
- submituje przez Jito gRPC,
- czeka na potwierdzenie zgodnie z aktualnym transport contract,
- przechodzi do jawnego stanu terminalnego.

### 4.4.6. Fail-closed terminalizacja

Żaden z poniższych przypadków nie może kończyć się już „miękkim zniknięciem” pozycji:

- brak entry price,
- brak monitoringu,
- brak armingu,
- brak ownera slotu,
- nieudane zbudowanie SELL,
- nieudany submit,
- nieudzone potwierdzenie.

Każdy taki przypadek musi kończyć się:

- jednoznacznym statusem terminalnym,
- przyczyną,
- logiem / metryką,
- zachowaniem zgodnym z fail-closed.

---

## 4.5. Inwarianty, które etap 1 musi wprowadzić

1. **Jeden potwierdzony BUY = jedna sesja exit.**
2. **Jedna sesja exit = jeden owner lifecycle.**
3. **Brak monitoringu bez utrwalonej realnej ceny wejścia.**
4. **Brak triggera biznesowego poza +30% / -30%.**
5. **Brak partial exits w proof-lane.**
6. **Brak paper fallbacku dla live lane.**
7. **Brak ShadowLedger jako źródła decyzji.**
8. **Brak release slotu przed terminalizacją.**
9. **Brak „successful-looking” zakończenia bez dowodu SELL albo jawnego fail-closed terminal reason.**
10. **Cały trigger decision path musi być odtwarzalny z: entry price + canonical live price.**

---

## 4.6. Testy i proof wymagane do zamknięcia etapu 1

### 4.6.1. Testy kontraktowe / integracyjne

Etap 1 musi dodać lub rozszerzyć pokrycie dla:

1. prawidłowego przejęcia ownership po `LiveConfirmed BUY`,
2. braku auto-release `ActivePositionLease` podczas handoffu,
3. poprawnego utrwalenia `entry_price` z potwierdzonego BUY tx metadata,
4. poprawnego wyliczania `+30%` i `-30%` od realnego `entry_price`,
5. uruchomienia **jednego** full SELL po trafieniu progu,
6. braku triggerowania z `ShadowLedger`,
7. jawnych terminal states dla fail-closed branchy,
8. zachowania direct handoff / broadcast handoff przy lag / close,
9. zachowania 1-2 równoległych pozycji w dual live bez utraty ownership.

### 4.6.2. Dowód runtime

Zamknięcie etapu 1 wymaga później runtime proofu:

1. uruchomienie 1-2 pozycji w `dual-micro-live`,
2. potwierdzony BUY landed,
3. zapis entry price,
4. monitoring canonical price,
5. aktywacja +30% lub -30%,
6. SELL 100% przez Jito gRPC bundle,
7. potwierdzony terminalny outcome dla każdej pozycji.

Nie wolno uznać etapu za zamknięty tylko na podstawie:

- paper proof,
- synthetic testów,
- samych eventów `PostBuySubmitted`,
- samych logów `Candidate`,
- shadow artefaktów bez potwierdzonego live SELL landed.

### 4.6.3. Obserwacje z realnego dual-live rollout do wykorzystania przy delegacji

Stan potwierdzony on-chain po pierwszym realnym podejściu:

1. `dual live` został uruchomiony na realnym configu produkcyjnym.
2. BUY transport przez Jito gRPC działa w praktyce.
3. Zostały potwierdzone **3 realne BUY landed**.
4. Dla tych pozycji **nie doszło do żadnego automatycznego SELL** z launchera.
5. Pozycje musiały zostać zamknięte ręcznie przez operatora.

To oznacza, że temat **nie jest już problemem samego live BUY**, tylko jest zawężony do:

- post-buy handoff do launcherowego live exit,
- aktywacji właściwej stage-1 ścieżki `LiveExit`,
- trigger/build/submit/confirm dla live SELL.

#### Co zostało już potwierdzone / wykluczone

1. Problem nie polega na braku salda lub braku podstawowej zdolności do wykonania live BUY.
2. Problem nie polega na nieprawidłowym profilu `dual`, bo rollout był uruchamiany po przejściu preflightu dla:
   - `execution_mode = "dual"`
   - `entry_mode = "live_and_shadow"`
3. Problem nie polega wyłącznie na samym ACK z Jito, bo przynajmniej część BUY została realnie zreconciliowana jako landed on-chain.
4. Sam fakt emisji `PostBuySubmitted` **nie jest jeszcze dowodem**, że aktywna stage-1 ścieżka SELL rzeczywiście przejęła lifecycle.

#### Kluczowa obserwacja operacyjna

W jednym z analizowanych retry-runów aktywny proces działał na **starej** binarce:

- `target/release/ghost-launcher` było starsze od bieżących zmian w `post_buy_runtime.rs`,
- logi z takiego runu nie są miarodajnym dowodem dla obecnej implementacji stage-1 live exit.

Wniosek praktyczny:

- przy dalszym debugowaniu należy zaczynać od **świeżego rebuilda release**,
- przed kolejnym runem trzeba jawnie potwierdzić, że uruchomiony proces zawiera nowe logi/stany `LiveExit:`.

#### Najważniejszy symptom do dalszego debugowania

Jeżeli po realnym BUY landed w logach widać:

- `PostBuyRuntime: received PostBuySubmitted`

a jednocześnie **nie** widać:

- `LiveExit: state transition`
- `LiveExit: persisted confirmed BUY entry metadata`

to oznacza to, że aktywny runtime **nie wszedł w właściwą stage-1 state machine**, nawet jeśli BUY został potwierdzony.

To jest sygnał do debugowania w tej kolejności:

1. czy uruchomiony proces jest na świeżej binarce,
2. czy `lane == "live"` faktycznie trafia do `run_live_sell_lifecycle(...)`,
3. czy `initialize_live_exit_session(...)` startuje i loguje przejścia stanu,
4. dopiero potem czy trigger + build + submit + confirm SELL działa poprawnie.

#### Artefakty i miejsca startowe do przejęcia tematu

Najbardziej użyteczne artefakty z dotychczasowych runów:

- `/root/.copilot/session-state/cffab6ea-16a6-4d39-97f6-6e3eb7d5698a/files/dual-live-artifacts/20260405T020602Z/launcher.stdout.log`
- `/root/.copilot/session-state/cffab6ea-16a6-4d39-97f6-6e3eb7d5698a/files/dual-live-artifacts/20260405T020602Z/manual-stop-stale-binary/`

Najważniejsze miejsca w kodzie do przejęcia debugowania:

- `ghost-launcher/src/components/post_buy_runtime.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/events.rs`
- `off-chain/components/trigger/src/jito_client.rs`

#### Aktualny status etapu 1 po tym rolloutcie

Po tym proofie etap 1 **nie może być uznany za zamknięty**, ponieważ:

1. istnieje dowód realnego BUY landed,
2. nie istnieje jeszcze dowód launcherowego BUY -> SELL landed w nowej architekturze,
3. blocker został zawężony do live SELL lifecycle i jego rzeczywistego uruchomienia po BUY.

---

## 4.7. Kryteria akceptacji etapu 1

Etap 1 jest zamknięty dopiero, gdy jednocześnie prawdziwe są wszystkie warunki:

1. po każdym potwierdzonym BUY system utrwala realny `entry_price`,
2. live price trigger działa canonical-first z `AccountStateCore`,
3. jedyne aktywne progi biznesowe to +30% / -30%,
4. wyjście oznacza SELL 100%,
5. slot ownership nie ginie w handoffie,
6. runtime nie kończy pozycji bez jawnego terminal reason,
7. live SELL dalej używa wyłącznie Jito gRPC bundle transport,
8. proof lane działa dla 1-2 równoległych pozycji w dual live,
9. istnieje rzeczywisty dowód BUY -> SELL landed w zmienionej architekturze.

---

## 5. TODO wykonawcze dla etapu 1

1. Zamrozić kontrakt etapu 1: single-position, full-exit, +30% / -30%, launcher-owned.
2. Wydzielić jawny live exit session/state machine dla proof-lane.
3. Podpiąć real entry-price extraction z confirmed BUY tx metadata.
4. Rozszerzyć handoff kontrakt o komplet danych wymaganych przez exit session, w tym landed slot BUY.
5. Uszczelnić ownership i release semantykę `ActivePositionLease` / `position_slot_id`.
6. Wprowadzić canonical-first monitoring ceny i wyliczanie `PnL` względem persisted entry price.
7. Zastąpić TP ladder proof-lane prostym full SELL path.
8. Dodać jawne terminal fail-closed states, metryki i logi dla całego lifecycle.
9. Dodać testy kontraktowe i integracyjne dla wszystkich krytycznych branchy etapu 1.
10. Wykonać controlled dual-live proof dla 1-2 pozycji.

---

## 6. Etapy dalsze

Kolejne etapy planu **nie są jeszcze rozwijane**. Zostaną dopisane dopiero po zamknięciu etapu 1 i po zebraniu realnego proofu z runtime.

Na dziś obowiązuje zasada:

> **najpierw domknąć architekturę i uproszczony produkcyjny full-exit path, dopiero potem rozwijać bardziej złożoną logikę SELL.**
