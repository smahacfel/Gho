# PLAN REDUKCJI LATENCJI BUY HOT PATH - 2026-04-09

## 0. Cel dokumentu

Ten dokument opisuje wykonawczy plan zejscia z obecnego czasu:

- `gatekeeper_verdict_at -> first buy_submitted_at = 411-676 ms`
- srednia z finalnego runu: `498.1 ms`

do poziomu realistycznie osiagnalnego w obecnej architekturze Sender:

- **cel roboczy:** `120-220 ms` dla sredniego `verdict -> first submit`
- **cel minimalny:** stale zejscie ponizej `300 ms`

Plan **nie** jest planem rezygnacji z obecnego Helius Sender transport.
Jesli kiedykolwiek powstanie osobny projekt transportowy (np. direct leader /
private orderflow / inny ultra-low-latency path), bedzie to osobna decyzja
architektoniczna i nie oznacza "powrotu do klasycznego publicznego TPU fallback".
Plan dotyczy wyciecia z obecnego BUY hot path zbednego I/O, przy zachowaniu
aktualnych kontraktow bezpieczenstwa, observability i SSOT.

---

## 1. Baseline z obecnego kodu i finalnego live runu

### 1.1. Co zmierzyly logi

Z finalnego runu `final-dual-live-20260409-123806`:

| Etap | Min | Max | Srednio |
|---|---:|---:|---:|
| `gatekeeper_verdict_at -> tip resolved` | 125 ms | 173 ms | 139.1 ms |
| `tip resolved -> prepared buy request accounts` | 201 ms | 345 ms | 249.3 ms |
| `prepared buy request accounts -> first buy_submitted_at` | 74 ms | 187 ms | 109.7 ms |
| `gatekeeper_verdict_at -> first buy_submitted_at` | 411 ms | 676 ms | 498.1 ms |

### 1.2. Co na pewno NIE jest problemem

W tym runie problemem nie byly:

- `IWIM` - disabled, efektywnie `0 ms`
- `local_preflight_latency_ms` - `0`
- `reserve_slot_latency_ms` - `0`
- `shadow_spawn_latency_ms` - `0`
- `client_setup_latency_ms` - `0`
- `slot_read_latency_ms` - `0`
- `blockhash_fetch_latency_ms` - `0` (cache hit)

### 1.3. Gdzie realnie znika czas

Obecny BUY hot path traci czas prawie w calosci na **seryjnych awaitach do HTTP/RPC**:

1. dynamiczny tip floor
2. fetch payer balance
3. fetch payer account
4. fetch mint account
5. probe user ATA
6. fetch pre-submit token balance lub ATA rent
7. priority fee estimate
8. drugi build transakcji po fee estimate

To nie jest problem CPU math. To jest problem sequencing i braku prewarm/cache.

### 1.4. Co pokazal kolejny dual-live po Fazie 6

Z runu `dual-live-20260412-234651` po wdrozeniu Faz 0-6:

- `tip_floor` nadal kosztowal ok. `118-138 ms` na kazdym BUY
- `priority_fee_fetch` nadal kosztowal ok. `50-69 ms` na kazdym BUY
- `build_once_ms` tam, gdzie byl widoczny, byl juz rzedu `1 ms`
- `gatekeeper_verdict_at -> first attempt/submitted_at` nadal mial ogon
  rzedu `257-293 ms` juz przed confirm tail

Wniosek:

- Faza 6 w wersji **advisory / fire-and-forget** byla poprawna kontraktowo,
  ale **nie domknela current-buy win**.
- Brakujacy element to **joinable prewarm / singleflight**, tak aby biezacy BUY
  mogl dolaczyc do juz rozpoczetego fetchu `tip_floor` albo `priority_fee`,
  zamiast wykonywac drugi, rownolegly albo opozniony fetch po swojemu.
- Bez tego optional Faza 4 bylaby skokiem do bardziej subtelnej optymalizacji,
  zanim zamkniemy bardziej oczywisty koszt `tip / fee HTTP`.

---

## 2. Scope i granice planu

### 2.1. In scope

1. `ghost-launcher/src/components/live_tx_sender.rs`
   - tip floor cache
   - priority fee cache / prewarm

2. `ghost-launcher/src/components/trigger/component.rs`
   - wyciecie zbednych fetchy z hot path
   - parallel prep RPC
   - payer keypair cache
   - uproszczenie ATA path
   - ograniczenie double-build

3. `ghost-launcher/src/oracle_runtime.rs`
   - prewarm hooki przed dispatch BUY

4. testy i telemetry
   - nowe checkpointy i cache hit/miss
   - testy regresyjne dla safety i confirmation semantics

### 2.2. Out of scope

1. zastapienie obecnego Helius Sender innym transportem execution-plane
   (np. direct leader / private orderflow / TPU path) - to bylby osobny projekt
   infra, a nie element tego planu
2. przebudowa Gatekeepera, IWIM, scoringu lub policy math
3. oslabienie confirmation semantics BUY/Sell
4. nowe "tymczasowe" legacy fallbacki
5. pre-creation ATA dla wszystkich kandydatow
6. hardcode rent values lub hardcode token-program assumptions

---

## 3. Nienegocjowalne kontrakty i SSOT

Ten plan ma byc wykonany bez lamania ponizszych zasad, chyba ze nastapi jawna,
osobna decyzja kontraktowa.

### 3.1. SSOT / architecture contracts

1. `LiveTxSender` pozostaje jedynym SSOT dla live Sender transport:
   - submit
   - dynamic tip floor policy
   - priority fee estimation path
   - Yellowstone confirmation

2. `oracle_runtime.rs -> BuyAccountOverrides` pozostaje metadata handoff surface.
   Nie tworzymy obok niego nowego mutable truth store dla BUY metadata.

3. `TriggerComponent::resolve_safe_trade_budget(...)` i emergency floor
   pozostaja canonical budget gate. Nic w tym planie nie moze rozluznic guardow.

4. `AccountStateCore`, `Gatekeeper`, `PoolObservationSession` i aktualna
   warstwa truth-source pozostaja poza zakresem zmian.

5. `blockhash cache` pozostaje aktualnym canonical source dla fast BUY blockhash.

### 3.2. Safety / observability contracts

1. BUY/Sell confirmation nie moze stac sie slabsza niz teraz.
2. `pre_submit_token_balance` ma pozostac prawdziwe i uzyteczne dla BUY confirm.
3. `priority_fee` ma pozostac dynamiczny; cache ma przyspieszac, nie zamrazac.
4. `payer_balance_lamports` dla `resolve_safe_trade_budget(...)` pozostaje fresh
   odczytem z hot path:
   - nie prewarmujemy go,
   - nie cache'ujemy go jako source-of-truth.
5. `pre_submit_token_balance = Some(0)` wolno ustawic tylko wtedy, gdy brak ATA
   jest potwierdzony przez probe o semantyce co najmniej rownowaznej obecnemu
   `user_ata_exists_with_retry(...)`:
   - z retry,
   - z secondary RPC tam, gdzie dzisiejszy kod go uzywa,
   - bez optimistic "single-source not found => ATA missing".
6. wszystkie nowe fast path musza byc telemetryzowane:
    - cache hit/miss
    - cache age
    - stale-last-good usage
    - fallback reason

### 3.3. Forbidden shortcuts

Nie wolno:

- zalozyc "ATA pewnie nie istnieje" bez dowodu,
- zalozyc "mint owner pewnie jest poprawny" bez planu walidacji,
- wyciac budget check tylko po to, aby BUY byl szybszy,
- potraktowac primary-only `Ok(None)` jako dowod braku ATA, jesli stary kontrakt
  wymagal secondary RPC / retry,
- mieszac BUY i SELL w jednym globalnym cache priority fee bez jawnego klucza
  klasy transakcji,
- prewarmowac albo cache'owac payer balance jako truth dla safety budget,
- dodac config flags, ktore sluza tylko do obchodzenia problemu.

---

## 4. Mapa obecnego hot path w kodzie

### 4.1. Oracle handoff

`ghost-launcher/src/oracle_runtime.rs`

- `execute_gatekeeper_buy_path(...)`
- `wait_for_live_trigger_readiness(...)`
- `resolve_live_buy_tip_lamports(...)`
- `derive_buy_account_overrides(...)`
- `execute_gatekeeper_buy_via_trigger(...)`

Istotny punkt:

- log: `"Shadow buy account overrides prepared"`

To jest naturalne miejsce na przyszly prewarm.

### 4.2. Trigger hot path

`ghost-launcher/src/components/trigger/component.rs:2508-2725`

Obecna sekwencja:

1. `load_payer()` - dzis czyta keypair z pliku przy kazdym BUY
2. `fetch_payer_balance_with_retry(...)`
3. `resolve_safe_trade_budget(...)`
4. `fetch_payer_account_with_retry(...)`
5. `fetch_mint_account_with_retry(...)`
6. `user_ata_exists_with_retry(...)`
7. `fetch_token_account_balance(...)` lub `minimum_user_ata_rent_lamports(...)`
8. `resolve_live_blockhash(...)`
9. pierwszy build z fallback priority fee
10. `estimate_priority_fee_micro_lamports(...)`
11. drugi build z dynamicznym priority fee

### 4.3. LiveTxSender hot path

`ghost-launcher/src/components/live_tx_sender.rs`

Obecnie:

- `fetch_tip_floor_lamports()` - **brak cache**
- `estimate_priority_fee_micro_lamports(...)` - **brak cache**

### 4.4. ATA builder

`ghost-launcher/src/components/trigger/component.rs:1766-1775`

To jest kluczowe:

- builder juz uzywa `create_associated_token_account_idempotent(...)`

Wniosek:

- nie musimy najpierw pytac sieci, czy instrukcja ATA create "wolno" byc dolaczona,
- mozemy uproscic probe ATA, o ile nie zepsujemy budget/confirmation semantics.

---

## 5. Strategia wykonawcza

Plan jest celowo rozbity na fazy od najnizszego ryzyka do najwyzszego ROI.
Nie robimy duzego refaktoru "za jednym zamachem".

### Faza 0 - telemetry freeze przed zmianami semantyki

**Cel:** najpierw zlapac twarde per-stage pomiary w kodzie, zeby po kazdej fazie
widziec realny zysk, a nie "wydaje sie szybciej".

#### Kroki

1. W `live_tx_sender.rs` dodac:
   - `tip_floor_cache_hit`
   - `tip_floor_cache_age_ms`
   - `tip_floor_fetch_latency_ms`
   - `priority_fee_cache_hit`
   - `priority_fee_cache_age_ms`
   - `priority_fee_fetch_latency_ms`

2. W `trigger/component.rs` dodac:
   - `payer_load_ms`
   - `payer_balance_fetch_ms`
   - `payer_account_fetch_ms`
   - `mint_account_fetch_ms`
   - `token_balance_probe_ms`
   - `ata_rent_fetch_ms`
   - `build_once_ms`
   - `rebuild_ms`

3. Dodac jeden czytelny log podsumowujacy:
   - np. `Trigger: BUY preparation breakdown`
   - z wszystkimi sub-latencies i cache flags

#### Kontrakt

- zero zmian behawioralnych
- zero zmian safety
- zero zmian confirm path

#### Po co ta faza

Bez tego kolejne fazy beda "oparte na intuicji". Ten plan ma byc mierzalny.

---

### Faza 1 - niskie ryzyko, natychmiastowy zysk

To sa rzeczy, ktore nie zmieniaja semantyki transakcji i nie dotykaja SSOT.

#### 1A. Cache keypair w `TriggerComponent`

**Dzis:** `load_payer()` czyta keypair z pliku przy kazdym BUY.

**Plan:**

1. w konstruktorze `TriggerComponent` zaladowac payer raz,
2. trzymac go w pamieci jako `Arc<Keypair>`,
3. usunac per-BUY `read_keypair_file(...)`.

**Kontrakt:**

- fail-fast pozostaje taki sam: jesli plik keypair jest zly, proces ma nie startowac
- brak hot reload klucza; zmiana klucza wymaga restartu, jak dotad

**ROI:** niski, ale darmowy i bezpieczny.

#### 1B. TTL cache dla tip floor w `LiveTxSender`

**Dzis:** kazdy BUY robi `GET https://bundles.jito.wtf/api/v1/bundles/tip_floor`.

**Plan:**

1. dodac `CachedTipFloor { lamports, fetched_at }`,
2. trzymac to w `LiveTxSender` jako wspoldzielony cache z interior mutability
   (`Arc<Mutex<_>>`, `Arc<RwLock<_>>` lub rownowazny mechanizm),
3. cache ma byc wspoldzielony przez wszystkie `Arc<LiveTxSender>` / clone tego
   sendera, a nie per-kopia structa,
4. wprowadzic krotki TTL (`250-500 ms`, stala w kodzie, nie nowy config),
5. jesli cache swiezy - uzyc cache,
6. jesli cache wygasl - odswiezyc i nadpisac,
7. jesli fetch padnie, a mamy bardzo swiezy `last_known_good`, wolno go uzyc z logiem
   `cache_mode="stale_last_good"` zamiast isc od razu do baseline.

**Kontrakt:**

- baseline min tip nadal obowiazuje
- brak cache == zachowanie jak dzis
- dynamic tip nie moze zniknac "po cichu"
- cache nie moze byc "martwy" przez clone-semantics `LiveTxSender`

**ROI:** ~`120-170 ms` na cache hit.

#### 1C. Cache ATA rent per token program

**Dzis:** przy nowym ATA lecimy po `getMinimumBalanceForRentExemption(...)`.

**Plan:**

1. dodac prosty cache `token_program -> rent_lamports`,
2. pobierac rent z RPC tylko na miss,
3. nie hardcodowac `2_074_080`.

**Kontrakt:**

- wartosc nadal pochodzi z RPC
- cache jest tylko przyspieszeniem, nie nowym SSOT

**ROI:** maly/sredni, ale prosty i bezpieczny.

#### 1D. Dynamic priority fee cache - najpierw jako safe accelerator

**Dzis:** kazdy BUY robi sync `getPriorityFeeEstimate`, potem rebuild.

**Plan:**

1. dodac cache `last successful BUY fee estimate`, ale tylko jako BUY-scoped
   accelerator:
   - pierwszy rollout nie moze mieszac BUY z SELL,
   - SELL moze pozostac on-demand do czasu osobnej decyzji,
2. kluczowac go co najmniej po:
   - `tx_kind = buy`
   - `buy_variant`
   - `token_program`
   - `ata_missing_pre_submit`
   - `has_inline_tip` (lub rownowaznej klasie builda)
3. Faza 1D jest twardo zalezna od Fazy 2 albo rownowaznego jawnego pola
   opisujacego klase ATA path; do tego czasu zostaje obecny path,
4. uzyc bardzo krotkiego TTL,
5. jesli cache swiezy - pominac sync estimate i uzyc cached dynamic fee,
6. jesli cache pusty/stary - zachowac obecny on-demand path jako slow path.

**Kontrakt:**

- to ma nadal byc dynamiczny fee, nie "zamrozony stale fee"
- pierwszy rollout ma zachowac obecny slow path jako fallback
- klucz cache nie moze mieszac BUY z SELL ani dwoch klas builda o innych
  instrukcjach / kontach

**ROI:** ~`60-120 ms` na cache hit + mniej rebuild churn.

---

### Faza 2 - uproszczenie ATA path bez lamania confirmation contract

To jest pierwszy etap, w ktorym trzeba bardzo pilnowac semantyki.

#### Problem

Dzis `create_user_ata` pelni kilka rol naraz:

1. steruje, czy dolaczamy instrukcje ATA create,
2. steruje budget check dla rent,
3. steruje tym, czy `pre_submit_token_balance` jest `0` czy realnym balansem.

To jest zbyt wiele znaczen dla jednego boola.

#### Fakty z kodu

1. builder juz umie zawsze dolaczyc `idempotent ATA create`,
2. `fetch_token_account_balance(...)` zwraca:
   - `Ok(Some(balance))` gdy ATA istnieje,
   - `Ok(None)` gdy ATA nie istnieje,
   - `Err(...)` dla realnego bledu RPC.
3. obecny `user_ata_exists_with_retry(...)` ma retry + secondary RPC semantics
   i to jest czesc dzisiejszego safety contract dla confirm fallback.

#### Plan

1. rozdzielic semantyke na dwa pojecia:
   - `attach_idempotent_ata_create`
   - `ata_missing_pre_submit`

2. dla live BUY path:
   - `attach_idempotent_ata_create = true` zawsze

3. probe ATA zredukowac do jednego logicznego helpera, niekoniecznie jednego
   surowego RPC:
   - dodac np. `probe_user_ata_pre_submit(...)`,
   - helper ma zwracac:
     - `ata_missing_pre_submit`
     - `pre_submit_token_balance`
     - `expected_ata_rent`
   - helper ma zachowac co najmniej rownowazna semantyke do dzisiejszego
     `user_ata_exists_with_retry(...)`:
     - retry,
     - secondary RPC fallback tam, gdzie dzisiejszy kod go uzywa,
     - brak optimistic "primary-only None => missing".
4. w helperze:
   - `Ok(Some(balance))`:
     - `ata_missing_pre_submit = false`
     - `pre_submit_token_balance = Some(balance)`
     - `expected_ata_rent = 0`
   - `Ok(None)`:
      - wolno ustawic `ata_missing_pre_submit = true` tylko wtedy, gdy helper
        potwierdzil brak ATA pod powyzszym kontraktem,
      - `pre_submit_token_balance = Some(0)`
      - `expected_ata_rent = cached_rent(token_program)`

5. jesli helper zwroci ambiguity / disagreement / prawdziwy blad RPC:
    - **nie zgadujemy**
    - w pierwszym rolloutcie schodzimy do konserwatywnego fallbacku:
      - stary probe path, albo
      - fail-closed prepare error
    - wybor rekomendowany:
      - stary probe path na exceptional / ambiguous result,
      - fail-closed, jesli nie umiemy odzyskac rownowaznej pewnosci.

#### Kontrakt

- BUY confirmation nadal dostaje prawdziwy `pre_submit_token_balance`
- budget check nadal wie, czy trzeba doliczyc rent
- nie dopuszczamy primary-only optimistic path dla ATA missing
- transport zyskuje, bo redukuje osobne RPC tylko wtedy, gdy safety contract
  zostaje zachowany

**ROI:** ~`20-50 ms` i uproszczenie kodu.

---

### Faza 3 - zrownoleglenie niezaleznych RPC w `prepare_buy_request()`

To jest najwiekszy czysty zysk bez ruszania SSOT.

#### Dzis

W `prepare_buy_request()` prawie wszystko idzie sekwencyjnie:

1. payer balance
2. payer account
3. mint account
4. ATA probe / token balance

#### Plan

1. po zaladowaniu payera odpalic niezalezne read-only fetchy przez `tokio::try_join!`
   lub rownowazny mechanizm:
   - `fetch_payer_balance_with_retry(...)`
   - `fetch_payer_account_with_retry(...)`
   - `fetch_mint_account_with_retry(...)` (dopoki jeszcze potrzebny)

2. jesli `account_overrides.token_program` jest obecny:
   - od razu wyliczyc `user_ata`
   - uruchomic spekulacyjny ATA probe rownolegle
   - po powrocie `mint_account` porownac canonical token program z override
   - przy mismatch:
     - odrzucic wynik spekulacyjnego ATA probe liczony dla zlego token program,
     - zrobic probe dla canonical token program albo zostac przy dzisiejszym
       konserwatywnym path,
     - fail-closed lub canonical overwrite tak jak dzis

3. zostawic obecne helpery retry bez przepisywania ich logiki na pierwszym kroku.
   Najpierw zmienic sequencing, nie policy.

4. `payer_balance_lamports` pozostaje fresh read w tym samym BUY hot path:
   - nie cache'ujemy go,
   - nie prewarmujemy go,
   - nie zastępujemy go zadnym stale snapshotem.

5. validation i sanitation wykonywac po zebraniu wynikow, tak jak dzis.

#### Kontrakt

- nic nie mutuje chain state
- helpery retry i error surface pozostaja te same
- nie usuwamy mint fetch "na wiare" w tej fazie
- safety budget nadal bazuje na swiezym payer balance

**ROI:** ~`100-170 ms`.

---

### Faza 4 - conditional mint fetch zamiast default mint fetch

Ta faza ma sens dopiero po telemetry proof.

#### Co juz wiemy

`derive_buy_account_overrides(...)` juz zbiera z runtime:

- `token_program`
- `global_config`
- `fee_recipient`
- `buy_variant`
- `associated_bonding_curve`

Czyli czesc danych potrzebnych do BUY juz przychodzi do triggera zanim wejdziemy
w `prepare_buy_request()`.

#### Problem

Nie wolno automatycznie uznac tego za nowy SSOT i wyrzucic `fetch_mint_account`
bez dowodu, ze runtime metadata jest canonical i stabilne.

#### Plan

1. w Fazie 0 telemetryzowac:
   - czy `account_overrides.token_program` jest obecny,
   - czy zgadza sie z `mint_account.owner`,
   - czy sa kiedykolwiek mismatch-e.
   - runtime proof ma byc czytelny w jednej metryce:
     `trigger_buy_token_program_validation_total{override_present,proof_result,source}`

2. dopiero po dowodzie z live telemetry:
   - jesli `token_program` jest obecny i w ciaglym oknie minimum:
     - `500` live BUY z `has_token_program=true`,
     - `72h` live runu,
     - `0` mismatchy `override_token_program != canonical_token_program`,
     - `trigger_buy_token_program_validation_total{override_present=\"true\",proof_result=\"mismatched\"} = 0`,
      wolno zrobic fast path:
      - skip `fetch_mint_account_with_retry(...)`
      - fallback do chain fetch tylko na missing override lub mismatch suspicion

3. ten etap powinien byc wdrazany osobno od Fazy 3.

#### Kontrakt

- runtime metadata nie staje sie nowym SSOT "bo tak wygodniej"
- chain-proof zostaje jako fallback / arbiter

**ROI:** dodatkowe `40-90 ms`, ale dopiero po dowodzie.

---

### Faza 5 - ograniczenie double-build i template reuse

To nie jest glowny winowajca, ale warto to domknac po I/O wins.

#### Dzis

W `prepare_buy_request()`:

1. budujemy tx z fallback fee,
2. pytamy o priority fee,
3. budujemy drugi raz ten sam request z nowa fee.

#### Plan

1. wprowadzic wewnetrzna reprezentacje typu:
   - `PreparedBuyTemplate`
   - albo `BuyBuildProfile`

2. ten template ma przechowywac:
   - sanitized overrides
   - token program
   - info o ATA path
   - min tokens out
   - tip account
   - stale instrukcje / kolejnosc kont

3. hot path ma:
   - pobrac dynamic fee z cache albo slow path
   - zrobic **jeden** final build + podpis

4. slow path z podwojnym buildem moze jeszcze istniec przez jeden rollout jako fallback.
5. `rebuild_prepared_buy_request_for_retry(...)` ma wejsc do scope tej fazy:
   - retry path ma uzywac tego samego kontraktu template/profile,
   - testy binarne maja objac zarowno initial BUY, jak i retry rebuild.

#### Kontrakt

- nie zmieniamy samej kolejnosci instrukcji w tx bez testow binarnych
- nie zmieniamy sign/submit contract
- roznice miedzy initial build i retry build maja ograniczac sie do tego, co
  jest intencjonalnie zmienne (`blockhash`, `tip`, `priority_fee`, podpisy)

**ROI:** glownie CPU / alloc / signing churn, rzad wielkosci `10-30 ms`.

---

### Faza 6 - prewarm z `oracle_runtime.rs`

To jest faza, ktora przenosi czesc kosztu **przed** finalny BUY dispatch.

#### Naturalne punkty zaczepienia

1. **Wczesny hook** - przed `resolve_live_buy_tip_lamports(...)`:
   - tylko ten punkt moze pomoc biezacemu BUY w kwestii `tip floor`.
2. **Pozny hook** - po logu `"Shadow buy account overrides prepared"`:
   - ten punkt jest dobry dla metadata-aware prewarm,
   - ale dla biezacego BUY jest juz za pozno na skracanie `resolve_live_buy_tip_lamports(...)`.

#### Plan

1. dodac jawne API typu `TriggerComponent::spawn_prewarm_advisory(...)`
   (lub rownowazny wrapper), ktore:
   - jest fire-and-forget,
   - nie blokuje BUY dispatch,
   - nie zwraca hard failure do callera,
   - nie tworzy nowego truth store.
2. z wczesnego hooka odswiezac tylko to, co moze pomoc biezacemu BUY:
   - `tip floor cache`.
3. z poznego hooka odswiezac tylko to, co ma sens po zebraniu metadata:
   - BUY-scoped priority fee cache dla prawdopodobnej klasy BUY.
4. oba prewarmy maja byc asynchroniczne i advisory dla elementow, ktore:
    - nie zmieniaja chain state,
    - nie sa budzetowo krytyczne,
    - nie tworza nowego truth store.

5. w pierwszym rolloutcie **nie** prewarmowac payer balance jako source-of-truth
   dla safety budget.
   Ten odczyt powinien pozostac aktualny i canonical w samym BUY path.

6. prewarm ma byc advisory:
   - brak prewarmu nie moze zlamac BUY
   - prewarm miss = normalny slow path
   - pozny hook moze pomoc kolejnych BUY lub fee cache dla biezacej klasy,
     ale nie wolno udawac, ze skraca biezacy `tip resolve`, jesli jest po nim.

#### Kontrakt

- zero side effects na chain
- zero nowego SSOT
- zero oslabenia emergency floor

**ROI:** wycina HTTP z krytycznego odcinka tylko tam, gdzie hook pojawia sie
przed danym fetch; w pozostalych miejscach przygotowuje cache dla kolejnego
odcinka albo kolejnego BUY.

### Faza 6B - RTM-coordinated joinable prewarm / singleflight

To jest faza domykajaca Faze 6 po telemetry proof z dual-live.

Cel:

- zamienic "advisory prewarm, ktory moze pomoc kolejnemu BUY" na
  **joinable prewarm, ktory realnie moze pomoc biezacemu BUY**
- usunac duplicate HTTP do `tip_floor` i `priority_fee`, jesli prewarm juz trwa

#### Plan

1. `LiveTxSender` dostaje jawny mechanizm **singleflight / join-or-start** dla:
   - `tip floor refresh`
   - `BUY priority fee refresh` keyed po tym samym `PriorityFeeCacheKey`,
     ktory juz chroni cache z Fazy 1D
2. ownership pozostaje w `LiveTxSender`:
   - sender pozostaje SSOT dla cache i refresh path
   - `TriggerComponent` i `oracle_runtime` moga tylko:
     - wystartowac refresh,
     - dolaczyc do refreshu,
     - odczytac telemetry wyniku
3. wczesny hook RTM nie moze juz tylko "odpalic taska":
   - musi otwierac `tip_floor` refresh na tyle wczesnie, aby biezacy BUY
     mogl dolaczyc do juz rozpoczetego fetchu
4. pozny hook metadata-aware dla priority fee ma wystartowac refresh klasy BUY
   po zebraniu metadata, ale hot path musi umiec dolaczyc do tego samego inflight
   requestu zamiast robic drugi fetch
5. `prepare_buy_request(...)` nie tworzy nowego fetchu, jesli istnieje zgodny
   inflight refresh:
   - dla `tip_floor` dolacza do inflight
   - dla `priority_fee` dolacza po `PriorityFeeCacheKey`
6. wait na inflight ma byc:
   - bounded,
   - telemetryzowany,
   - fail-open do dzisiejszego slow path, jesli inflight jest zbyt pozny albo
     nieudany

#### Kontrakt

- zero nowego SSOT poza `LiveTxSender`
- zero prewarm/caching dla `payer_balance_lamports`
- zero mieszania BUY i SELL we wspolnym inflight key
- zero rozluznienia safety/confirm semantics
- jesli inflight nie pomaga biezacemu BUY, hot path ma zachowac obecny fallback

#### ROI

To jest faza, ktora ma realnie wyciac:

- `~120 ms` z `tip_floor`
- `~50-70 ms` z `priority_fee_fetch`

nie tylko dla kolejnego BUY, ale dla **biezacego** `verdict -> first submit`.

---

## 6. Proponowana kolejnosc wdrozenia

Rekomendowana kolejnosc jest nienegocjowalna, jesli chcemy malego blast radius:

1. **Faza 0** - telemetry only
2. **Faza 1A + 1B + 1C** - payer cache, tip cache, ATA rent cache
3. **Faza 3** - parallel prep RPC
4. **Faza 2** - ATA path collapse z zachowaniem confirm contract
5. **Faza 1D** - dynamic priority fee cache
6. **Faza 5** - single-build/template reuse
7. **Faza 6** - prewarm z oracle runtime (dopiero gdy 1B i 1D maja gotowe API cache)
8. **Faza 6B** - RTM-coordinated joinable prewarm / singleflight
9. **Faza 4** - optional mint-fetch elision dopiero po telemetry proof z Faz 6 + 6B

Powod:

- najpierw bierzemy szybkie i bezpieczne ms,
- dopiero potem dotykamy bardziej subtelnych semantyk ATA / metadata truth,
- `1D` zalezy od jawnej semantyki `ata_missing_pre_submit`,
- `6` zalezy od istnienia bezpiecznych cache API oraz dwoch poprawnych hookow,
- `6B` zalezy od telemetry proof z Fazy 6 i zamienia advisory prewarm na current-buy
  latency win bez lapania nowego SSOT,
- optional fast path bez `mint fetch` ma byc ostatni, nie pierwszy, i dopiero po
  domknieciu `tip / fee` jako glownych awaitow.

---

## 7. Dokladne punkty zmian w kodzie

### 7.1. `ghost-launcher/src/components/live_tx_sender.rs`

Do dodania:

1. struktury cache:
   - `CachedTipFloor`
   - `CachedPriorityFee`
   - `PriorityFeeCacheKey`

2. pola w `LiveTxSender`:
   - shared cache storage z interior mutability
   - timestamps

3. nowe helpery:
   - `get_cached_tip_floor(...)`
   - `store_cached_tip_floor(...)`
   - `get_cached_buy_priority_fee(...)`
   - `store_cached_buy_priority_fee(...)`
   - `join_or_start_tip_floor_refresh(...)` lub rownowazny singleflight helper
   - `join_or_start_buy_priority_fee_refresh(...)` lub rownowazny singleflight helper
   - opcjonalnie `refresh_*_if_stale(...)`
   - helpery BUY priority fee nie moga mieszac sie z SELL path bez jawnego
      `tx_kind` w kluczu
   - inflight BUY priority fee musi byc keyed co najmniej tak samo scisle jak cache
     z Fazy 1D

4. telemetry:
   - hit/miss
   - age
   - fetch latency
   - stale-last-good usage
   - cache mode / source
   - inflight join hit/miss
   - inflight wait duration
   - duplicate fetch avoided / not avoided

### 7.2. `ghost-launcher/src/components/trigger/component.rs`

Do zmiany:

1. `TriggerComponent` constructor / fields
   - cached payer
   - ewentualny ATA rent cache jesli nie trzymamy go w senderze
   - brak cache dla payer balance

2. `prepare_buy_request(...)`
   - parallel fetch stage
   - uproszczenie ATA probe przez jeden logiczny helper z zachowaniem secondary
     RPC semantics
   - usuniecie zbednej sekwencyjnosci
   - discard spekulacyjnego ATA wyniku przy token_program mismatch
   - dolaczenie do `tip_floor` / `priority_fee` inflight refresh, jesli RTM juz
     je wystartowal
   - bounded fallback do obecnego slow path, jesli inflight nie zdazyl pomoc

3. `PreparedBuyRequest`
   - rozdzielenie znaczen dzisiejszego `create_user_ata`
   - doprecyzowanie pola dla budget/confirm semantics
   - w rolloutcie przejsciowym wolno utrzymac `create_user_ata` jako derived
     compatibility field, ale canonical semantyka ma byc jawna

4. `build_buy_transaction(...)`
   - finalnie zawsze z idempotent ATA create dla live BUY path
   - ale z prawidlowym budget/confirm metadata obok

5. `rebuild_prepared_buy_request_for_retry(...)`
   - ten sam template / build profile contract
   - brak semantycznego driftu wzgledem initial BUY poza intencjonalnymi polami

6. logi:
   - `BUY preparation breakdown`
   - cache flags
   - `ata_missing_pre_submit`
   - `priority_fee_source`
   - `prewarm_joined_current_buy`
   - `prewarm_only_helped_next_buy`

### 7.3. `ghost-launcher/src/oracle_runtime.rs`

Do dodania:

1. advisory prewarm API wywolywane przez `TriggerComponent`
2. wczesny hook przed `resolve_live_buy_tip_lamports(...)` dla tip cache
3. pozny hook po `Shadow buy account overrides prepared` dla BUY priority fee cache
4. telemetry:
    - prewarm started
    - prewarm hit/miss
    - prewarm age at BUY dispatch
    - czy prewarm mogl pomoc biezacemu BUY czy tylko kolejnemu odcinkowi
5. po telemetry proof z Fazy 6:
    - hook tip prewarm przeniesiony / otwarty dostatecznie wczesnie, aby current BUY
      mogl dolaczyc do juz trwajacego inflight refresh
    - hook priority fee uruchamiajacy inflight keyed po klasie BUY, a nie tylko
      "best effort cache warm"

---

## 8. Testy i walidacja

### 8.1. Unit / integration tests do dodania lub zaktualizowania

1. `LiveTxSender`
   - tip floor cache hit
   - tip floor cache miss -> fetch -> store
   - stale-last-good tylko przy fetch failure
   - cache jest wspoldzielony przez clone / `Arc<LiveTxSender>`
   - priority fee cache keyed by BUY class
   - priority fee cache nie miesza BUY z SELL
   - brak regresji parsera decimal priority fee
   - dwa rownolegle `tip_floor` resolve dla tego samego momentu lacza sie w jeden
     HTTP fetch
   - dwa rownolegle BUY priority fee resolve dla tej samej klasy BUY lacza sie w
     jeden HTTP fetch
   - dwa rozne `PriorityFeeCacheKey` nie dziela jednego inflight refresh
   - failed / timed out inflight nie blokuje kolejnych callerow i nie zostawia
     stalego locka

2. `TriggerComponent`
   - payer keypair nie jest czytany z pliku per BUY
   - payer keypair nie jest czytany z pliku per retry rebuild
   - ATA missing -> `pre_submit_token_balance=Some(0)` + rent doliczony
   - ATA existing -> realny pre-submit balance zachowany
   - `ata_missing_pre_submit` staje sie `true` tylko po probe o rownowaznej
     semantyce retry / secondary RPC
   - disagreement / ambiguity w ATA probe nie prowadzi do optimistic path
   - idempotent ATA create nadal obecny w BUY tx
   - parallel prep zwraca te same sanitized account overrides i safety checks
   - spekulacyjny ATA probe jest odrzucany przy token_program mismatch
   - unexpected token balance RPC error nie prowadzi do cichego optimistic path
   - payer balance fetch pozostaje fresh read w hot path
   - retry rebuild zachowuje ten sam binary contract co initial BUY poza
      `blockhash`, `tip`, `priority_fee` i podpisami
   - current BUY potrafi dolaczyc do rozpoczetego przez RTM `tip_floor` refresh
   - current BUY potrafi dolaczyc do rozpoczetego przez RTM `priority_fee` refresh
     dla zgodnej klasy BUY
   - jesli RTM prewarm jest zbyt pozny, slow path pozostaje bez regresji correctness
   - nadal brak payer balance prewarm jako truth source

3. BUY/Sell confirmation
   - brak regresji `balance_delta` semantics
   - brak regresji signature-status / Yellowstone assist path
   - brak nowego wzrostu falszywych `balance_delta` confirm przez ATA probe

### 8.2. Repo validation

Po kazdej fazie:

1. `cargo build -p ghost-launcher --quiet`
2. `cargo test -p ghost-launcher --quiet`

Jesli faza dotyka tylko telemetry i helperow, nie ma powodu uruchamiac wiekszego
zakresu niz juz istniejace targety.

### 8.3. Live validation sequence

Po kazdej fazie z realna zmiana hot path:

1. rollout preflight
2. krotki controlled dual live run
3. porownanie:
   - `gatekeeper_verdict_at -> first buy_submitted_at`
   - `gatekeeper_verdict_at -> buy_confirmed_at`
   - orphan count
   - priority fee fallback count
   - confirm_source mix (`yellowstone` / `signature_status` / `balance_delta`)
   - stale state reject count
   - emergency floor behavior
   - brak regresji live SELL build/confirm po zmianach w `LiveTxSender`
4. przed rozpoczeciem Fazy 4 proof gate dla Faz 6 + 6B:
   - `tip_floor` musi miec potwierdzony current-buy join rate > 0 i materialny
     spadek hot-path `tip_floor_fetch_latency_ms`
   - `priority_fee` musi miec potwierdzony current-buy join rate > 0 i materialny
     spadek hot-path `priority_fee_fetch_latency_ms`
   - duplicate fetch rate dla tych dwoch fetchy musi spasc wzgledem runu po Fazie 6
   - jesli to nie zachodzi, nie przechodzimy do Fazy 4

---

## 9. Acceptance criteria

Plan mozna uznac za skutecznie dowieziony dopiero wtedy, gdy jednoczesnie
spelnione sa wszystkie warunki:

1. sredni `gatekeeper_verdict_at -> first buy_submitted_at <= 220 ms`
2. brak regresji BUY/Sell confirmation semantics
3. `priority_fee` pozostaje dynamiczny, a nie fallback-only
4. emergency floor nadal zatrzymuje run fail-closed
5. brak nowej klasy orphan BUY wynikajacej z fast path
6. logi pokazuja cache hit ratio i rzeczywisty breakdown
7. brak regresji live SELL wynikajacej ze wspoldzielonych zmian w `LiveTxSender`
8. po Fazach 6 + 6B mamy telemetry proof, ze `tip_floor` i `priority_fee` nie sa juz
   dominujacymi seryjnymi awaitami dla current BUY
9. jesli Faza 4 zostanie wlaczona:
    - telemetry proof gate (`500` BUY / `72h` / `0` mismatchy) pozostaje spelniony

Jesli po Fazach 1-6B srednia nadal pozostaje wyraznie powyzej `220-250 ms`,
kolejnym logicznym krokiem nie jest "dalszy mikrotuning Rust", tylko zmiana
warstwy transportowej / infra.

---

## 10. Czego ten plan nie obiecuje

Ten plan **nie** obiecuje:

1. `10 ms verdict -> submit`
2. `10 ms submit -> confirmed on-chain`
3. HFT-grade latencji na publicznym RPC / Sender path

Ten plan ma zrobic to, co jest sensowne w obecnym stosie:

- wyciac oczywisty dlug,
- przyspieszyc BUY o setki ms, nie o pojedyncze ms,
- zrobic to bez psucia kontraktow i bez udawania, ze Sender == colocated TPU HFT.

---

## 11. Oczekiwany efekt koncowy po wdrozeniu faz 1-6

Realistyczny target po domknieciu planu:

| Etap | Dzis | Cel po planie |
|---|---:|---:|
| `verdict -> tip resolved` | ~139 ms | `<20 ms` na cache hit |
| `tip resolved -> prepared accounts` | ~249 ms | `80-140 ms` |
| `prepared accounts -> first submit` | ~110 ms | `40-80 ms` |
| `verdict -> first submit` | ~498 ms | `120-220 ms` |

To jest pulap, ktory da sie osiagnac bez lamania obecnych kontraktow.
Zejsciem do ligi `10-50 ms` zajmuje sie juz inna klasa infrastruktury.
