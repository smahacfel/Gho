# Wykonawczy plan domknięcia PR-7 i uruchomienia market-driven paper-live sim do 2026-04-10

## Cel dokumentu

Ten plan ma dowieźć dwa konkretne rezultaty przed **2026-04-10**:

1. **market-driven paper-live sim bez realnego kapitału**, w którym Ghost:
   - podejmuje realne decyzje BUY z aktualnego ingestu,
   - otwiera pozycję jak w runtime rolloutowym,
   - prowadzi ją na aktualnych danych rynkowych,
   - używa logiki `Revolver` + post-buy managera,
   - zamyka pozycję po spełnieniu TP / panic / time-stop,
   - daje dokładny P&L tej pozycji;
2. **uczciwe domknięcie PR-7**, czyli pierwszy kontrolowany `dual + live_and_shadow` z pełnym dowodem:
   - shadow vs live,
   - net P&L,
   - operational loss,
   - continue / abort.

Nie rozbijamy tego na dziesiątki małych PR. Program ma się zamknąć w **6 rzeczowych PR-ach**.

---

## Stan zweryfikowany na 2026-03-28

### 1. Jesteśmy po formalnym zaliczeniu PR-6, ale przed domknięciem PR-7

Zweryfikowane lokalnie:

- `.ghost/baseline_accepted_revision` wskazuje ten sam commit co aktualne `HEAD`.
- `python3 scripts/shadow_run_report.py --config configs/rollout/paper-burnin.toml --metrics-text logs/rollout/paper-burnin/metrics.prom`
  zwrócił:
  - `VERDICT: GO`
  - `paper_lifecycle_complete: PASS`
  - `trace_correlation: PASS`
  - `economics_not_fatal: PASS`

Wniosek: **paper burn-in jest formalnie zielony** i można przechodzić do prac PR-7.

### 2. Artefakty PR-7 z planu z 2026-03-25 istnieją, ale merge gate nie jest spełniony

Istnieją dziś:

- profile rolloutowe:
  - `configs/rollout/paper-burnin.toml`
  - `configs/rollout/dual-micro-live.toml`
  - `configs/rollout/future-live.toml`
- runbook i preflight:
  - `docs/RUNBOOK_PRODUCTION_ROLLOUT.md`
  - `scripts/ghost_production_preflight.sh`
- artefakty prób dual:
  - `logs/rollout/dual-micro-live/*`
  - `datasets/events/dual-micro-live/*`
  - `logs/shadow_run/dual-micro-live-buys.jsonl`

### 3. Aktualny status operational readiness dla `dual-micro-live`

Aktualna walidacja preflightu:

- `transport.grpc`: OK
- `trigger.rpc_url`: OK
- `trigger.keypair`: OK
- `trigger.jito_endpoint`: OK
- `trigger.balance`: OK
- jedyny twardy fail z preflightu to:
  - `metrics.port: 9092 already in use`

To nie jest już blocker architektoniczny, tylko **operacyjny konflikt aktywnego runa**.

### 4. Najważniejsze: obecny dual nie daje jeszcze prawdy, której wymaga PR-7

Zweryfikowane artefakty pokazują następujący stan:

- w `datasets/events/dual-micro-live/*` **live lane zawiera głównie `Candidate`**, bez pełnego `EntrySubmitted -> PositionClosed`;
- nie znaleziono `live`-lane eventów typu:
  - `EntrySubmitted`
  - `EntryFilled`
  - `PositionOpened`
  - `ExitSubmitted`
  - `ExitFilled`
  - `PositionClosed`
- jednocześnie pełny lifecycle pojawia się w lane `single`, z identyfikatorami typu:
  - `paper-*`
  - `paper-pos-*`

To oznacza, że obecny dual:

- **nie daje uczciwego shadow vs live execution trace**,
- **nie daje uczciwego live P&L trace**,
- miesza w jednym worku:
  - live candidate lane,
  - paper-like lifecycle lane.

### 5. Obecny paper lifecycle nie spełnia celu „idealnego live bez kapitału”

W aktualnym kodzie:

- `ghost-brain/src/execution/paper_lifecycle.rs` prowadzi cenę przez `synthetic_mark_price(...)`;
- `estimated_costs_sol = 0.0`;
- `ghost-launcher/src/components/post_buy_runtime.rs` mapuje `execution_mode = dual` na `Lane::Single`;
- `ghost-brain/src/guardian/post_buy/engine.rs` ma lepszy, real-market manager oparty o `ShadowLedger` i `Revolver`, ale nie jest dziś SSOT dla paper-live rolloutu;
- `PositionClosed` w części ścieżek nadal może kończyć bez pełnego `gross/net/operational` accounting.

Wniosek: **repo ma już dużo gotowych klocków, ale nie ma jeszcze jednego kanonicznego market-driven paper-live position engine**.

---

## Weryfikacja kryteriów zaliczenia PR-7 z planu z 2026-03-25

### Kryteria już spełnione

- istnieje kanoniczny profil `dual + live_and_shadow`;
- istnieją rollout docs, preflight i profile;
- istnieją realne artefakty sesji dual z 2026-03-28;
- Jito submit path nie jest już w stanie „zawsze martwy” z powodu starego błędu URL;
- safety / WAL / preflight są realnie obecne w runtime.

### Kryteria niespełnione lub spełnione tylko częściowo

1. **Każdy realny BUY ma dawać realny execution trace + shadow trace**
   - dziś: **nie**
   - live lane w artefaktach nie ma pełnego lifecycle execution.

2. **Możliwość policzenia divergence fill quality**
   - dziś: **nie**
   - brak spójnego zestawu live `EntryFilled/ExitFilled` skorelowanego z paper/shadow.

3. **Każdy trade kończy się uczciwym wynikiem netto**
   - dziś: **nie**
   - paper ma synthetic mark path i `estimated_costs_sol = 0.0`;
   - live/accounting trace nie jest jeszcze kanonicznie domknięty.

4. **Pełny raport po mikro-live: shadow vs live, net P&L, operational loss, continue/abort**
   - dziś: **nie**
   - repo ma report dla paper burn-in, ale nie ma jeszcze zamkniętego raportu PR-7 z tym kontraktem.

5. **System po mikro-live umie odpowiedzieć, czy divergence execution jest akceptowalna**
   - dziś: **nie**
   - brakuje pełnego dual compare SSOT.

Wniosek końcowy: **PR-7 nie jest domknięty**. Jesteśmy po zielonym PR-6 i w środku prac PR-7, ale bez finalnego dowodu.

---

## Docelowy kontrakt: czego dokładnie chcemy

Docelowy run bez realnego kapitału ma działać tak:

1. Ghost dostaje realny ingest i podejmuje realną decyzję BUY.
2. Zamiast wysyłać realny BUY, tworzy **wirtualną pozycję** z wejściem opartym o:
   - shadow fill / quote,
   - albo jawnie uchwycony market quote.
3. Od tej chwili pozycja jest prowadzona na **aktualnych danych rynkowych**, nie na synthetic path.
4. Zarządzanie pozycją wykorzystuje:
   - `PostBuy` manager,
   - `AEM`,
   - `Revolver` albo jego wirtualny odpowiednik z tymi samymi targetami co live.
5. Gdy TP / panic / time-stop zadziała, następuje **virtual sell**:
   - z ceną z chwili wyjścia,
   - albo z jawnie zrobionym backfillem po fakcie, jeśli capture w chwili sell się nie uda.
6. Każda pozycja kończy się pełnym recordem:
   - entry time / price / size,
   - exit time / price / size,
   - gross P&L,
   - fees / tip / slippage / op-loss,
   - net P&L,
   - close reason.

To jest próg „Ghost symuluje live uczciwie”, którego dziś jeszcze nie ma.

---

# Program wykonawczy

## PR-1 — `dual-lane-truth-and-rollout-ssot`

### Twardy cel

Uczynić artefakty rolloutowe semantycznie prawdziwymi i usunąć operator ambiguity.

### Zakres

1. Rozdzielić lane semantics w dual:
   - paper lifecycle ma emitować `lane = paper`,
   - live execution ma emitować `lane = live`,
   - `Lane::Single` nie może być już kanonicznym lane dla dual rollout evidence.
2. Usunąć paper-only identyfikatory z dual proof path:
   - brak `paper-*` jako jedynego execution śladu w dual compare.
3. Naprawić albo jawnie ujednoznacznić kontrakt sekretów:
   - dziś env override działa tylko dla placeholderów;
   - docs i preflight muszą raportować **rzeczywiste źródło** każdej wartości krytycznej.
4. Dodać lepszy preflight dla konfliktów portów:
   - fail ma mówić, który proces trzyma port,
   - bez zgadywania przez operatora.

### Główne pliki

- `ghost-launcher/src/components/post_buy_runtime.rs`
- `ghost-brain/src/events/*`
- `ghost-launcher/src/config.rs`
- `scripts/ghost_production_preflight.sh`
- `docs/SECRET_HYGIENE_AND_ROLLOUT_PROFILES.md`

### Merge gate

- krótki rehearsal dual tworzy dwa rozłączne strumienie:
  - `live`
  - `paper`
- preflight pokazuje jawnie aktywne źródło walleta / endpointów;
- occupied port nie jest już „ślepym” błędem.

---

## PR-2 — `market-driven-paper-position-engine`

### Twardy cel

Zastąpić synthetic paper lifecycle rzeczywistym market-driven managementem bez realnego kapitału.

### Zakres

1. Wyjąć rolloutowy paper-live spod `synthetic_mark_price(...)`.
2. Oprzeć paper-live ticki o aktualne dane:
   - `ShadowLedger` snapshoty,
   - `ShadowLedgerPriceOracle` z RPC fallback,
   - realny timestamp ticka.
3. Uczynić `ghost-brain::guardian::post_buy::MonitoringEngine` kandydatem na SSOT dla market-driven position management.
4. Wprowadzić jawny rollout profile / flagę:
   - np. `paper-live-sim`,
   - albo `execution.paper.market_driven = true`.

### Główne pliki

- `ghost-brain/src/execution/paper_lifecycle.rs`
- `ghost-brain/src/guardian/post_buy/engine.rs`
- `ghost-launcher/src/components/post_buy_runtime.rs`
- `off-chain/components/trigger/src/revolver_price_feed.rs`

### Merge gate

- w rollout path nie ma synthetic price managementu;
- paper-live pozycja reaguje na realny market movement, a nie lokalną funkcję deterministyczną.

---

## PR-3 — `virtual-revolver-and-paper-exit-truth`

### Twardy cel

Dać paper-live ten sam exit contract co live: targety, partiale, panic, time-stop i zamknięcie pozycji po realnej cenie rynkowej.

### Zakres

1. Po virtual/paper entry ładować **virtual magazine** z real entry price.
2. Użyć tej samej polityki targetów co live `Revolver`.
3. Na ticku rynkowym:
   - sprawdzać target,
   - emitować `ExitSubmitted`,
   - emitować `ExitFilled`,
   - wspierać partial exits.
4. `PostBuy` manager i `RevolverAemAdapter` mają działać na tym samym stanie pozycji.

### Główne pliki

- `ghost-brain/src/guardian/post_buy/engine.rs`
- `off-chain/components/trigger/src/revolver.rs`
- `off-chain/components/trigger/src/revolver_integration.rs`
- `off-chain/components/trigger/src/revolver_worker.rs`

### Merge gate

- paper-live potrafi zamknąć pozycję po TP / panic / time-stop bez side effectu on-chain;
- event trace pokazuje dokładny sell moment, fraction i reason.

---

## PR-4 — `entry-exit-price-proof-and-pnl-ledger`

### Twardy cel

Każda zamknięta pozycja paper/live ma mieć dokładny i audytowalny wynik netto.

### Zakres

1. Zbudować wspólny ledger accountingowy dla paper i live:
   - entry value,
   - exit value,
   - gross pnl,
   - fee,
   - Jito tip,
   - slippage / execution loss,
   - operational anomalies,
   - net pnl.
2. Live BUY:
   - brać entry z `EntryPriceExtractor` po potwierdzeniu transakcji.
3. Sell price truth:
   - łapać cenę w momencie sell triggera,
   - jeśli capture nie doszedł, backfill przez jawny skrypt po handlu.
4. Wypełnić `PositionClosedPayload` bez `None` dla rollout lanes.

### Główne pliki

- `off-chain/components/trigger/src/entry_price_extractor.rs`
- `ghost-brain/src/events/schema.rs`
- `ghost-brain/src/events/emitter.rs`
- `ghost-brain/src/guardian/post_buy/engine.rs`
- nowy skrypt w `scripts/` do backfillu / repairu sell-price

### Merge gate

- każda zamknięta pozycja w paper-live i dual ma pełne:
  - `entry_value_sol`
  - `exit_value_sol`
  - `gross_pnl_sol`
  - `net_pnl_sol`
  - `estimated/explicit_costs`
- użytkownik może policzyć dokładny P&L z samych artefaktów.

---

## PR-5 — `dual-compare-proof-and-session-report`

### Twardy cel

Domknąć właściwy kontrakt PR-7: jedna sesja ma dać dowód `decision -> shadow -> paper-live -> live`.

### Zakres

1. Spiąć korelację po `candidate_id` / `position_id` dla:
   - decision,
   - shadow buy,
   - live buy,
   - paper-live position,
   - sell / close.
2. Zbudować formalny raport dual:
   - shadow vs live,
   - paper-live vs live,
   - divergence fill quality,
   - net pnl,
   - operational loss,
   - continue / abort.
3. Wymusić, że live lane bez execution events nie przechodzi raportu.

### Główne pliki

- `scripts/shadow_run_report.py` albo nowy dedykowany report script
- `ghost-brain/src/events/comparison.rs`
- `ghost-launcher/src/events.rs`
- `ghost-brain/src/events/*`

### Merge gate

- raport dual powstaje z aktualnych artefaktów bez ręcznego sklejania;
- brak pełnego live execution trace oznacza automatyczne `NO-GO`.

---

## PR-6 — `proof-run-closure-and-2026-04-10-package`

### Twardy cel

Zamknąć cały program jednym pakietem dowodowym, a nie „wrażeniem z terminala”.

### Zakres

1. Przygotować dwa finalne profile:
   - `paper-live-sim`
   - `dual-micro-live`
2. Ustalić operacyjną sekwencję:
   - rehearsal,
   - paper-live proof run,
   - dual proof run.
3. Zarchiwizować komplet artefaktów do decyzji sponsorsko-produktowej:
   - logs,
   - events,
   - shadow trace,
   - metrics,
   - raport sesji.
4. Odpowiedź końcowa ma być binarna:
   - `CONTINUE_CAUTIOUSLY`
   - albo `ABORT_AND_FIX`

### Główne pliki

- `configs/rollout/*`
- `docs/RUNBOOK_PRODUCTION_ROLLOUT.md`
- `scripts/ghost_production_preflight.sh`
- raporty z `scripts/`

### Merge gate

- market-driven paper-live sim daje pełny lifecycle i exact P&L;
- dual proof run daje pełny compare trace i recommendation;
- można złożyć sponsorom jeden pakiet dowodowy zamiast opisu słownego.

---

## Kolejność i dyscyplina

Kolejność jest obowiązkowa:

1. PR-1 `dual-lane-truth-and-rollout-ssot`
2. PR-2 `market-driven-paper-position-engine`
3. PR-3 `virtual-revolver-and-paper-exit-truth`
4. PR-4 `entry-exit-price-proof-and-pnl-ledger`
5. PR-5 `dual-compare-proof-and-session-report`
6. PR-6 `proof-run-closure-and-2026-04-10-package`

Nie wolno:

- zaczynać od Jito gRPC migration jako głównej ścieżki programu;
- udawać, że obecny paper lifecycle już symuluje live;
- uznać PR-7 za zamknięty tylko dlatego, że runtime wysłał bundle UUID;
- traktować `Candidate`-only live lane jako execution proof.

---

## Najkrótszy wniosek operacyjny

Na dziś Ghost jest **po zaliczeniu paper burn-in**, ale **przed faktycznym domknięciem PR-7**.

Największy brak nie jest już w samym BUY submit path. Największy brak jest w tym, że:

- paper nie jest jeszcze market-driven live sim,
- dual nie daje jeszcze uczciwego live execution trace,
- końcowy P&L nie jest jeszcze jednolicie dowodowy.

Jeżeli mamy dowieźć rezultat przed **2026-04-10**, to najsensowniejsza ścieżka nie brzmi:

> „jeszcze trochę dłubać przy przypadkowych fixach”

tylko:

> „zrobić jeden kanoniczny market-driven paper-live engine, spiąć go z Revolverem i accountingiem, a potem domknąć PR-7 raportem dual”.
