# Plan wykonawczy `RUG_SCALP_V2`

**Data aktualizacji:** 2026-07-21  
**Status:** READY_FOR_IMPLEMENTATION_AFTER_PR75_BASE_PIN  
**Następca:** `PLAN_RUG_SCALP_V1_PROSPECTIVE_SHADOW_20260719.md`

---

## 0. Decyzja i zakres

```text
PROSPECTIVE_ONLY
SHADOW_ONLY
POSITION_MANAGER_IS_SOLE_LIFECYCLE_OWNER
NO_HISTORICAL_ARCHIVE
NO_ENTITY_RESOLUTION
NO_FUNDING_GRAPH
NO_ML
NO_GATEKEEPER_AUTHORITY_CHANGE
NO_LIVE_EXECUTION_AUTHORIZED
MAX_DATA_COLLECTION = 48 H
```

Cel:

> Sprawdzić, czy pozycja `0,10 SOL`, otwierana w trakcie nadal aktywnej bardzo wczesnej sekwencji zakupów, osiąga `+10%` netto przed pierwszym materialnym dumpem albo zanikiem sekwencji.

`RUG_SCALP` nie jest klasyfikatorem oszustów. Nie rozstrzyga, kto kontroluje portfele. Wybiera obserwowalny stan rynku:

- buy burst trwa w dwóch kolejnych slotach;
- sprzedaż jeszcze się nie rozpoczęła;
- nasz trade jest mały względem ostatniego flow;
- do targetu potrzeba małego dalszego flow względem flow już obserwowanego.

Możliwe wyniki:

```text
REJECTED
REJECTED_LOW_CAPACITY
SHADOW_EDGE_CANDIDATE
```

PASS pozwala wyłącznie przygotować osobny micro-live canary plan.

---

## 1. Warunek rozpoczęcia po PR #75

Implementacja RUG SCALP musi bazować na **dokładnym finalnym commitcie PR #75 zawierającym ukończony Position Manager**, a nie na wcześniejszym `main`.

Przed utworzeniem brancha zapisać:

```text
implementation_base_commit
position_manager_code_hash
position_manager_config_hash
release_binary_hash
rug_scalp_config_hash
```

Obowiązkowy baseline audit:

1. potwierdzić, że nowy Position Manager przyjmuje izolowaną pozycję shadow/probe;
2. potwierdzić, że jest jedynym właścicielem lifecycle i decyzji zamknięcia;
3. potwierdzić, że research position nie zajmuje primary live/shadow capacity;
4. zachować fail-closed blokady `RouteUnsupported`, `RouteUnknown`, missing identity/state i stale executable quote;
5. potwierdzić oddzielenie freshness obserwacji od faktycznej aktywności rynku;
6. potwierdzić dokładnie jeden lifecycle i jedno terminalne zamknięcie na `position_id`.

RUG SCALP nie wymaga promocji Position Managera do live authority. Wystarcza kompletna, deterministyczna ścieżka observe-only/shadow.

Jeżeli PR #75 nie jest jeszcze na `main`, nie wolno implementować RUG SCALP na starym `main`. Branch należy utworzyć dopiero z finalnego SHA PR #75 albo z jego merge commita.

---

## 2. Hipoteza

```text
H0: E[R_net_primary_0p10] <= 0
H1: E[R_net_primary_0p10] > 0
```

`R_net` jest zwrotem po:

- programowych fee;
- price impact;
- base i priority fees;
- rzeczywistym/modelowanym Jito tipie;
- zamrożonej latencji wejścia i wyjścia;
- failed attempts;
- kosztach ATA tylko w części rzeczywiście nieodzyskanej.

Mechanizm V2:

1. burst zakupowy trwa co najmniej dwa kolejne sloty;
2. bieżący slot nadal niesie znaczące buy flow;
3. przed wejściem nie wystąpiła udana sprzedaż;
4. pozycja `0,10 SOL` jest mała względem burstu;
5. powtórzenie najwyżej połowy ostatniego dwuslotowego flow wystarczyłoby do `+10%` netto.

Nie testujemy psychologii, tożsamości ani break-even organizatora.

---

## 3. Universe i dane

Wyłącznie prospective:

```text
mainnet-beta
Pump bonding curve
SOL-paired
standard
non-Mayhem
non-cashback
curve open
curve niemigrowana
```

Poza zakresem:

```text
PumpSwap
USDC
historia sprzed runu
sociale
holder/funding/creator history
wallet clustering
```

Użyć istniejących canonical producers.

Potrzebne pola:

```text
Birth:
  mint
  curve
  birth_slot
  monotonic ingress time
  mode flags

Successful Pump trade:
  user
  side
  slot
  tx/instruction order
  monotonic ingress time
  effective quote amount
  token amount
  canonical reserves before/after

Execution/lifecycle:
  current fee schedule
  base/priority fee
  actual/modelled Jito tip
  build/send timing
  simulation/landing status
  executable buy and sell quote
```

Telemetryka CU/fingerprint może być zapisywana jako diagnostyka, ale nie gatuje V2. Nie tworzymy drugiego scoring stacku.

Każdy mint:

```text
maksymalnie jeden signal intent
maksymalnie jedna pozycja primary
zero re-entry
```

---

## 4. Sizing

Primary:

```text
N_primary = 0,10 SOL całkowitego spendu wejścia
```

Capacity sensitivity:

```text
N_sensitivity = 0,20 SOL
```

Zasady:

1. tylko `0,10 SOL` tworzy primary shadow lifecycle i decyduje o PASS/REJECT;
2. `0,20 SOL` jest liczony kontrfaktycznie na tych samych stanach;
3. sensitivity nie tworzy drugiej pozycji, nie zwiększa accepted count i nie zmienia rynku;
4. finalny raport podaje `max_validated_notional`:
   - `0,10 SOL`, jeżeli przechodzi tylko primary;
   - `0,20 SOL`, jeżeli sensitivity również ma dodatnie EV i przechodzi impact/latency gates.

Wszystkie wartości `0,004`, `0,05` oraz kalkulacje kosztowe oparte na tych nominalach są poza kontraktem V2.

---

## 5. Sygnał `RUG_SCALP_SIGNAL_V2`

Sygnał oceniamy natychmiast po każdym successful BUY w slocie `s`.

```text
s0 = birth slot
W  = slot s-1 oraz slot s
```

Dla `W`:

```text
n_prev     = buy count w s-1
n_curr     = buy count w s
n_2        = n_prev + n_curr
u_2        = unique successful TradeEvent.user w W
V_prev     = effective quote-in w s-1
V_curr     = effective quote-in w s
V_2        = V_prev + V_curr
top1_share = największy udział jednego usera w V_2
sell_seen  = jakikolwiek successful sell od birth
age_ms     = signal ingress - birth ingress
```

`u_2` i `top1_share` nie oznaczają niezależności ekonomicznej. Są wyłącznie ochroną przed jednym pojedynczym widocznym orderem udającym burst.

Na bieżącym canonical state liczymy program-exact osobno dla `0,10` i `0,20 SOL`:

```text
self_impact(N)
Q_TP(N) = minimalny dodatkowy effective buy flow,
          po którym exact executable sell pozycji N
          daje R_net >= +10%
```

`Q_TP` wyznacza deterministyczny binary search używający tej samej authoritative quote math co runtime. Nie wolno tworzyć osobnego uproszczonego modelu krzywej.

### Warunek primary entry

```text
age_ms <= 5 000
s > s0

n_prev >= 2
n_curr >= 2
n_2 >= 6
u_2 >= 4

V_2 >= 0,50 SOL
V_curr >= 0,50 * V_prev
top1_share <= 0,40

sell_seen = false

N_primary <= 0,20 * V_2
self_impact(N_primary) <= 1,00%
Q_TP(N_primary) <= 0,50 * V_2

canonical state quality = CLEAN
canonical ordering = KNOWN
no accepted-window event gap
```

`V_2 >= 0,50 SOL` nie jest niezależnym arbitralnym progiem: wynika z wymogu, aby primary position `0,10 SOL` stanowiła najwyżej 20% ostatniego flow.

Pierwszy event spełniający całość tworzy signal.

Mint staje się terminalnie niekwalifikowalny po:

```text
pierwszym successful sellu
age > 5 s
completion/migration
nieusuwalnej luce danych
wcześniejszym sygnale
```

Znaczenie sygnału:

> Burst trwa teraz, nasz trade jest mały, target jest blisko względem ostatniego flow, a dystrybucja jeszcze nie wystąpiła.

---

## 6. Entry i integracja z Position Managerem

Po signal:

1. zapisać typed `RugScalpEntryAssessmentV2`;
2. zapisać config/code/Position Manager hash;
3. natychmiast wysłać candidate do istniejącej izolowanej shadow/probe entry lane;
4. użyć istniejącego route resolvera oraz authoritative Pump buy buildera;
5. nie zmieniać Gatekeeper/V3 verdictu ani active BUY;
6. utworzyć dokładnie jeden `position_id`;
7. zarejestrować pozycję w nowym Position Managerze z:
   ```text
   strategy_id = "rug_scalp_v2"
   exit_profile_id = "rug_scalp_exit_v1"
   ```
8. utrzymać canonical state/trades do terminalnego zamknięcia.

### Zakaz drugiego właściciela wyjścia

`RugScalpSignalReducerV2`:

```text
może emitować evidence
może emitować entry assessment
nie może zamknąć pozycji
nie może prowadzić własnego lifecycle
nie może emitować terminalnego exit verdictu
```

Position Manager pozostaje jedynym właścicielem:

```text
position state
exit intent
retry
confirmation/reconciliation
terminal close
```

### Strategy profile wewnątrz Position Managera

Nie tworzymy nowego managera. Dodajemy minimalny, typed profile do istniejącego managera.

Precedence:

```text
0. PENDING / RECONCILIATION
1. IDENTITY / DATA / ROUTE BLOCKERS
2. MATERIAL_SELL_EMERGENCY
3. TARGET_REACHED_10PCT_NET
4. BASELINE_HARD_LOSS_5PCT_NET
5. FLOW_EXHAUSTED
6. MAX_HOLD
7. HOLD
```

Dla `rug_scalp_exit_v1` wyłączyć:

```text
partial exits
trailing profit
recovery-aware vitality tuning
adaptive target
re-entry
```

Nie zmienia to innych profili Position Managera.

---

## 7. Latencja

Założenie `+1 slot` musi zostać potwierdzone w smoke z aktualną prywatną instancją RPC i geolokalizowanym routingiem Jito.

Przed Run A zamrażamy:

```text
primary_entry_latency_slots
primary_exit_latency_slots
```

Reguła:

```text
jeżeli smoke p90 decision→reachable landing <= 1 slot:
  PRIMARY = +1 slot
w przeciwnym razie:
  PRIMARY = najmniejsza pełna liczba slotów pokrywająca p90
```

Stress:

```text
STRESS_1 = PRIMARY + 1 slot
STRESS_2 = PRIMARY + 2 sloty
```

Entry jest wyceniane na pierwszym canonical state osiągalnym po frozen latency, nie na stanie sygnału.

TP, material sell i stop są wyceniane na stanie osiągalnym po exit latency. Dotknięcie poziomu przed hipotetycznym landingiem nie jest fill’em ani zwycięstwem.

Brak wystarczającej latency telemetry po smoke oznacza:

```text
BLOCKED_NO_LATENCY_EVIDENCE
```

a nie optymistyczne `0 slot`.

---

## 8. Exit profile `rug_scalp_exit_v1`

Pozycję ocenia canonical Position Manager po każdym trade/account update.

### 8.1 Take-profit

Exit intent przy pierwszym exact executable sell quote dającym:

```text
R_net >= +10,00%
```

Wynik liczymy po frozen exit latency.

### 8.2 Materialny sell

Sell albo sekwencja selli w jednym slocie jest materialna, gdy:

```text
spadek real_sol_reserves >= 5% rezerwy sprzed sekwencji
LUB
spadek executable value naszej pozycji >= 15%
```

Tworzy `MATERIAL_SELL_EMERGENCY`.

Jeżeli target i dump występują w tym samym slocie, a kolejności naszego landing nie da się dowieść:

```text
DUMP_WINS
```

### 8.3 Hard stop

```text
R_net executable <= -5%
```

### 8.4 Flow stop

```text
dwa kolejne kompletne sloty bez successful BUY
```

Slot z luką danych nie jest pustym slotem. Luka daje data blocker/fail-closed, nie `FLOW_STOP`.

### 8.5 Max hold

```text
wcześniejszy z:
  8 slotów od entry
  5 000 ms od entry
```

### 8.6 Route/data failure

`RouteUnsupported`, `RouteUnknown`, stale quote i niejednoznaczny state pozostają typed blockerami zgodnie z Position Managerem. Nie wolno obchodzić ich przez timeout lub synthetic sell price.

Pełna pozycja, bez partial TP, trailing i re-entry.

---

## 9. PnL i outcome

```text
R_net =
(
  confirmed/modelled net SOL z wyjścia
  - całkowity SOL debited przy wejściu
  - base fees
  - priority fees
  - Jito tips
  - nieodzyskany ATA rent
  - failed attempt costs
)
/ N_intended
```

```text
N_intended_primary = 0,10 SOL
N_intended_sensitivity = 0,20 SOL
```

Fee zawarte w program quote liczymy dokładnie raz. Mark price jest niedopuszczalny.

Unknown entry nie otwiera pozycji bez dowodu fillu. Unknown exit nie księguje odzyskania kapitału bez evidence zgodnego z Position Managerem.

Primary metric:

```text
NET_ATTEMPT_RETURN_0P10
```

Secondary:

```text
NET_ATTEMPT_RETURN_0P20_COUNTERFACTUAL
TP_BEFORE_MATERIAL_DUMP
MATERIAL_DUMP_BEFORE_TP
SAME_SLOT_DUMP
TIME_TO_TP
TIME_TO_DUMP
ENTRY/EXIT SUCCESS RATE
MFE/MAE EXECUTABLE
PRIMARY VS STRESS LATENCY
MAX_VALIDATED_NOTIONAL
```

---

## 10. Minimalna implementacja

### Jeden PR, domyślnie wyłączony

```text
rug_scalp_v2.enabled = false
```

Zakres:

1. `RugScalpSignalConfigV2`.
2. Pure `RugScalpSignalReducerV2` z canonical successful Pump trades.
3. Exact self-impact oraz `Q_TP` dla `0,10` i `0,20 SOL` na authoritative quote math.
4. Typed entry assessment i reason codes.
5. Adapter `signal -> existing isolated shadow/probe entry`.
6. `rug_scalp_exit_v1` jako profil istniejącego Position Managera.
7. Retencja canonical state/trades do terminalnego close.
8. Deterministyczny evaluator primary oraz sensitivity PnL.
9. Jeden rollout config.
10. Testy.
11. Jeden ADR wymagany przez repo.

Nie dodawać:

```text
nowego Position Managera
nowego Gatekeepera/verdictu
drugiego lifecycle
ML
funding/holder graphu
historycznego importera
PumpSwap
```

---

## 11. Konfiguracja zamrażana przed runem

```toml
[rug_scalp_v2]
enabled = false
mode = "observe_only"

max_birth_age_ms = 5000
min_prev_slot_buys = 2
min_current_slot_buys = 2
min_two_slot_buys = 6
min_two_slot_unique_users = 4
min_two_slot_effective_quote_sol = 0.50
min_current_to_previous_quote_ratio = 0.50
max_top1_quote_share = 0.40
require_zero_sells_before_entry = true

primary_position_size_sol = 0.10
sensitivity_position_size_sol = 0.20
max_position_to_recent_flow_ratio = 0.20
max_entry_self_impact_bps = 100
profit_min_net_bps = 1000
max_required_flow_to_recent_flow_ratio = 0.50

material_sell_reserve_drain_bps = 500
material_sell_position_value_drop_bps = 1500
hard_stop_net_bps = -500
max_hold_slots = 8
max_hold_ms = 5000
flow_stop_empty_slots = 2

primary_entry_latency_slots = "<freeze_after_smoke>"
primary_exit_latency_slots = "<freeze_after_smoke>"
stress_extra_latency_slots_1 = 1
stress_extra_latency_slots_2 = 2

one_signal_per_mint = true
reentry_enabled = false
position_manager_profile = "rug_scalp_exit_v1"
```

Po rozpoczęciu Run A żadnej wartości nie zmieniamy.

---

## 12. Artefakty

```text
rug_scalp_births_v2.jsonl
rug_scalp_signal_assessments_v2.jsonl
rug_scalp_probe_entries_v2.jsonl
rug_scalp_pm_decisions_v2.jsonl
rug_scalp_position_events_v2.jsonl
rug_scalp_outcomes_v2.jsonl
rug_scalp_run_report_v2.json
```

Signal row:

```text
run/candidate/mint identity
birth i signal slot/time
n_prev, n_curr, n_2, u_2
V_prev, V_curr, V_2
top1_share
sell_seen
curve state hash
self-impact 0.10/0.20
Q_TP 0.10/0.20
assessment/reason
config hash
code commit
Position Manager hash
```

Outcome row:

```text
signal identity
position_id
entry intent/landing/status/cost/tokens
Position Manager decision chain
TP/material-sell/hard/flow/max-hold intent
exit landing/reconciliation
exit reason
net SOL
R_net 0.10
counterfactual R_net 0.20
first material sell
same-slot dump
MFE/MAE
primary/stress latency outcomes
```

Nie zapisujemy pełnych bloków ani wielomiesięcznej historii.

---

## 13. Testy przed runem

### Unit/property

```text
dwuslotowe buy counters
unique TradeEvent.user
flow persistence
top1 share
sell invalidation
age cutoff
one signal per mint

exact self-impact 0.10/0.20
exact Q_TP 0.10/0.20
fee counted once

exactly one Position Manager lifecycle
reducer cannot close a position
route/data blockers cannot be skipped
TP on landing state
same-slot dump beats exit
material-sell aggregation
hard/flow/max-hold exits
duplicate/reordered event handling
missing state => NON_EVALUABLE
flag disabled => zero side effects
```

### Integration

```text
birth
→ trades
→ signal
→ isolated shadow entry
→ Position Manager registration
→ retained states
→ one terminal close
→ primary and sensitivity outcome

zero zmian Gatekeeper verdictu
zero zmian primary capacity
zero duplicate exit authority
writer gaps jawne
clean shutdown
```

### Resource

```text
no hot-path RPC
reducer p99 <= 1 ms/event
bounded queue
zero runtime paniców
zero dropped canonical events dla accepted attempts
```

---

## 14. Protokół runu

### Smoke techniczny

```text
maksymalnie 2 h
```

Smoke służy wyłącznie do:

- parser/state coverage;
- signal rows;
- authoritative quote parity;
- PM profile lifecycle;
- entry/exit latency;
- writer i reconciliation;
- koszt/PnL accounting.

PnL smoke nie może zmienić progów.

Po smoke zamrażamy:

```text
implementation commit
release binary hash
config hash
Position Manager hash
primary latency slots
```

### Run A

```text
24 pełne godziny
```

Po 24 h:

```text
<10 completed primary attempts:
  REJECTED_LOW_CAPACITY

10–29:
  Run B tylko gdy wynik nie jest jednoznacznie ujemny

>=30:
  ocena primary gates
```

Hard reject przy `n >= 30`, jeżeli:

```text
one-sided 95% upper bound mean R_net_0p10 <= 0
LUB
mean < 0 AND median < 0 AND 20% trimmed mean < 0
LUB
STRESS_1 mean <= 0
```

### Run B

Tylko po braku rejectu Run A:

```text
kolejne 24 pełne godziny
nowy run_id
identyczny code/config/PM hash
zero tuningu
```

Maksymalny właściwy capture:

```text
48 h
```

---

## 15. Finalne bramki

### `SHADOW_EDGE_CANDIDATE`

```text
>=50 completed primary attempts łącznie
>=10 w każdym runie

mean R_net_0p10 > 0 w obu runach
combined mean, median i 20% trimmed mean > 0
one-sided 90% bootstrap lower bound mean > 0

STRESS_1 mean > 0
najlepszy trade <= 35% całego positive PnL

zero gaps dla accepted attempts
zero runtime paniców
zero duplicate/ambiguous terminal closes
```

Sizing:

```text
jeżeli 0.20 sensitivity mean > 0
i STRESS_1 dla 0.20 > 0
i self-impact <= 1%:
  max_validated_notional = 0.20 SOL
w przeciwnym razie:
  max_validated_notional = 0.10 SOL
```

### `REJECTED`

```text
combined primary mean <= 0
Run B primary mean <= 0
median/trimmed mean pokazuje wynik zależny od outlierów
STRESS_1 odwraca EV
same-slot/material dump losses zjadają targety
route/entry/exit failures zjadają EV
```

### `REJECTED_LOW_CAPACITY`

```text
<10 completed primary attempts w 24 h
```

Nie przedłużamy eksperymentu do tygodni.

---

## 16. Raport końcowy

Jedna tabela i jeden JSON:

```text
births
signal accepts
entry attempts/success
completed attempts
TP/material-sell/same-slot/hard/flow/max-hold
mean/median/trimmed R_net 0.10
mean/median/trimmed R_net 0.20 sensitivity
one-sided CI
MFE/MAE
time-to-TP
time-to-dump
primary/stress latency
positive-PnL concentration
max_validated_notional
final verdict
```

Bez AUC, graduation rate, modeli i wielostronicowego research reportu.

---

## 17. Rollback i kolejność

Feature flag default `false`.

Observe-only profile:

- nie rezerwuje live capacity;
- nie może zatrzymać Ghosta;
- nie zmienia aktywnego Gatekeepera;
- używa jednego canonical Position Managera;
- overflow/writer gap unieważnia run, ale nie runtime.

Kolejność:

```text
1. Potwierdzenie finalnego PR75 SHA / merge SHA.
2. Baseline audit Position Managera.
3. Jeden PR RUG_SCALP_V2.
4. Testy.
5. Smoke <=2 h i freeze latency.
6. Run A 24 h.
7. REJECT albo Run B 24 h.
8. Final verdict.
9. Po PASS wyłącznie osobny micro-live canary plan.
```

---

## 18. Kontrakt końcowy

```text
SIGNAL =
  ACTIVE TWO-SLOT BUY BURST
  + NO SELL YET
  + PRIMARY 0.10 SOL SMALL VS FLOW
  + +10% TARGET CLOSE TO RECENT FLOW

ENTRY =
  FIRST REACHABLE CANONICAL STATE AFTER SIGNAL

SOLE LIFECYCLE OWNER =
  CURRENT POSITION MANAGER FROM PR75

EXIT =
  FIRST REACHABLE STATE AFTER:
    MATERIAL SELL
    OR +10% NET TP
    OR -5% HARD STOP
    OR FLOW STOP
    OR 8-SLOT / 5-S MAX HOLD

SAME-SLOT DUMP = LOSS

PRIMARY NOTIONAL = 0.10 SOL
SENSITIVITY / CAPACITY = 0.20 SOL

MAX CAPTURE = 48 H

NO HISTORY
NO ENTITY RESOLUTION
NO ML
NO SECOND POSITION MANAGER
NO AUTHORITY CHANGE
