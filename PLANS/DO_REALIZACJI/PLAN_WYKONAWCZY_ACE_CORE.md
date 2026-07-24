Plan wykonawczy walidacji ACE Core — 0,15 SOL / minimum +17% netto

Rola dokumentu: bezpośrednio implementowalny plan uzyskania pierwszego wiarygodnego werdyktu executable EV dla ACE Core
Repozytorium: "smahacfel/Gho"
Data weryfikacji: 24 lipca 2026 r.
Zweryfikowany baseline: "origin/main = a12ef9cfb7199d44841cde27be2ecd8af13e2f3f"
Stan PR: PR #79 zmergowany; PR #78 pozostaje otwartym Draftem i nie jest zależnością ACE Core
Akcja: wyłącznie "ENTER_CONTINUATION" kontra "NO_ENTRY"
Kapitał wejścia: maksymalny całkowity wallet debit "150_000_000" lamportów
Profit exit: sprzedaż całej pozostałej pozycji z landed executable net return co najmniej "+17%"
Maksymalna liczba PR: dwa średnie PR-y
Runtime authority: bez zmian
Punkt STOP: jeden końcowy werdykt określony w rozdziale 17

«Wszystkie wskazania "file:line" odnoszą się do commit-u
"a12ef9cfb7199d44841cde27be2ecd8af13e2f3f", chyba że wskazano nowy plik.»

---

0. Decyzja wykonawcza

ACE Core nie staje się nowym Gatekeeperem, nowym Position Managerem ani równoległym decision plane.

Implementacja pozostaje zamknięta w dwóch PR-ach:

PR A
  brakujące cutoff-safe cechy w MaterializedFeatureSet
  + exact real reserves
  + minimalne istniejące evidence potrzebne do:
      - odtworzenia curve path,
      - zbudowania komponentowego cost ledgeru,
      - odróżnienia trigger/submit/landing/failure,
      - zbudowania route-agnostic transport episodes

PR B
  jeden offline typed executable replay
  + V1 safety envelope
  + executable-net profit gate +17%
  + failure, retry, migration i POSITION_STUCK accounting
  + jeden fixed conditional-mean model
  + sekwencyjny test max_concurrent_positions = 1
  + jeden root verdict

Plan odpowiada wyłącznie na pytanie:

«Czy pięć cutoff-safe cech ACE Core pozwala wybierać wejścia "ENTER_CONTINUATION", które przy maksymalnym całkowitym wallet debit 0,15 SOL i pełnopozycyjnym wyjściu chroniącym minimum +17% netto generują dodatni, wykonalny EV?»

Nie badamy:

- notionalu 0,004 SOL;
- alternatywnych notionali;
- mark-price profit;
- graduation;
- abstrakcyjnego "P(continuation)";
- event-by-event optimal stopping;
- routingu ACE kontra RUG SCALP;
- HET-PM V2 jako primary policy;
- PumpSwap rescue;
- partial exits;
- Kelly sizingu;
- portfolio optimization;
- live authority.

Zamrożony przepływ:

full Pump/SOL birth universe
  → jeden terminalny cutoff MFS
  → dokładnie pięć cech
  → initial feasibility dla pełnego capu 0,15 SOL
  → typed BuyV2 trigger-state quote
  → time-local route-agnostic transport episode
  → typed BuyV2 landing-state settlement
  → V1 safety lifecycle + offline executable-net profit gate 17%
  → typed LegacySell trigger/landing settlement
  → component-wise costs, failures, retries, migration
  → POSITION_STUCK semantics
  → jeden frozen mean-EV model
  → sequential max-one-position untouched test
  → jeden root verdict

---

1. Zweryfikowany stan repozytorium

1.1. Baseline

Przed rozpoczęciem implementacji należy wykonać:

git fetch origin
git rev-parse origin/main

Oczekiwany SHA w chwili sporządzenia planu:

a12ef9cfb7199d44841cde27be2ecd8af13e2f3f

Jeżeli "origin/main" uległ zmianie, implementator:

1. nie rozpoczyna pracy na starym SHA;
2. weryfikuje wskazane call sites;
3. aktualizuje "file:line";
4. zachowuje wszystkie kontrakty niniejszego planu;
5. nie rozszerza zakresu tylko dlatego, że zmienił się baseline.

PR #78 nie jest importowany, cherry-pickowany ani wymagany przez ACE Core.

1.2. Repozytoryjny shadow notional nie jest ekonomią ACE

"config.toml:79-116" zawiera obecnie między innymi:

trigger.max_position_size_sol = 0.004
trigger.entry_mode = shadow_only
trigger.max_concurrent_positions = 1
trigger.slippage_tolerance = 0.25

Znaczenie:

- "0,004 SOL" pozostaje parametrem zwykłego shadow runtime;
- nie jest fallbackiem;
- nie jest baseline’em modelu;
- nie jest sensitivity;
- nie może pojawić się w execution contract ACE;
- "25%" nie jest dopuszczalnym ACE impact ani counterfactual-validity limitem;
- "max_concurrent_positions = 1" jest obowiązującym ograniczeniem sekwencyjnego testu.

Capture profile nie zmienia runtime position size na "0,15 SOL". Ekonomia 0,15 istnieje wyłącznie w offline execution contract.

1.3. Typed Pump contract

PR #79 dostarcza typed:

BuyV2
LegacySell
ProgramStateTransition
ProgramSettlement
TransactionCosts
RuntimeProgramFeeScheduleRegistryV1

Typed route contract rozdziela:

program debit/credit
transaction-envelope costs

"BuyV2.max_sol_cost" ogranicza wyłącznie program wallet debit. Nie obejmuje:

- base fee;
- priority fee;
- Jito tip;
- ATA rent;
- kosztów wcześniejszych failed attempts.

Dlatego limit 0,15 SOL jest egzekwowany przez ACE ponad typed quote.

Aktualne ordinary call sites nadal używają:

DirectBuyBuilder
SellTxBuilder

ACE replay bada zatem jawnie:

typed BuyV2 / LegacySell contract

Nie wolno nazywać wyniku pełną parity z obecnym ordinary DirectBuilder runtime.

1.4. V1 i HET-PM V2

V1 jest minimalnym, czystym safety/lifecycle envelope:

StopLoss
TakeProfit
Inactivity
AbsoluteMaxHold
quote recovery
full-position exit

Obecny V1 TakeProfit jest mark-based i nie modeluje pełnych kosztów. Zostaje wyłączony z primary ACE replay.

HET-PM V2 implementuje odmienną hierarchię:

Crash
→ HardLoss
→ ExecutableTrailing
→ VitalityDecay
→ AbsoluteMaxHold

Nie jest stałym executable-net take-profitem. Pozostaje poza primary policy ACE.

1.5. MFS i canonical session

"PoolObservationSession" pozostaje właścicielem:

- canonical admission;
- deduplikacji;
- bounded "tx_buffer";
- TxIntelligence;
- checkpointów;
- decision-time series;
- terminalnej materializacji.

"MaterializedFeatureSet" pozostaje jedynym authoritative snapshotem cech.

Nie powstaje:

LiveDecisionFrame
AceDecisionSnapshot
drugi mutable reducer
drugi właściciel feature state

1.6. Exact reserves

Parser Pump zna:

virtual_token_reserves
virtual_sol_reserves
real_token_reserves
real_sol_reserves
complete

Obecny transport i launcherowy relay tracą real reserves. Typed "PumpReserveState" wymaga wszystkich czterech.

PR A musi przenieść exact real reserves jako addytywne evidence, bez zmiany authority "AccountStateCore".

---

2. Nienaruszalne inwarianty

1. "MaterializedFeatureSet" pozostaje SSOT cech decyzji.
2. ACE nie zmienia "BUY/REJECT/PENDING/TIMEOUT".
3. ACE nie emituje "EntryIntent".
4. ACE nie wywołuje Triggera.
5. ACE nie rejestruje pozycji w runtime PM.
6. ACE nie wykonuje BUY ani SELL.
7. PR A nie liczy score, EV ani candidate flag.
8. PR B jest offline binary/scriptem niewpiętym do launchera.
9. Dokładnie pięć cech jest używanych przez model.
10. Notional, route, impact, capacity, freshness i execution evidence są deterministic contractem, nie predyktorami.
11. Quote nie jest submit.
12. Submit nie jest landing.
13. Landing nie jest automatycznie successful program settlement.
14. Unknown execution nie jest sukcesem.
15. Brak confirmed full-position SELL nie zamyka pozycji.
16. POSITION_STUCK nie zwalnia capacity.
17. Późniejszy exit impact jest zawarty w typed settlement, ale nie stanowi osobnego veto.
18. Execution episode musi być wcześniejszy od ocenianego attemptu.
19. Route-specific failure innego buildera nie może zostać odziedziczone przez typed ACE route.
20. Koszty są rozliczane komponentowo, nie jednym booleanem.
21. Migration po entry nie znika z denominatora.
22. Nie wolno ratować niewykonalnej pozycji mniejszym notionalem.
23. Model nie jest ponownie fitowany po otwarciu untouched testu.
24. Negatywny albo nieważny wynik jest prawidłowym końcem planu.

---

3. Universe, cutoff i dokładnie pięć cech

3.1. Universe

Denominator:

wszystkie poprawnie zaobserwowane mainnet Pump/SOL birth events
w zamrożonym capture period

Universe nie może pochodzić z:

- BUY rows;
- Gatekeeper PASS;
- lifecycle positions;
- listy migracji;
- listy tokenów z pełnym outcome;
- model candidates;
- popular/active-token API.

Każdy birth otrzymuje jeden status:

EVALUABLE
NON_EVALUABLE_FEATURES
NON_EVALUABLE_CAPACITY
NON_EVALUABLE_ROUTE
NON_EVALUABLE_EXECUTION
INVALID_CAPTURE
INVALID_ECONOMICS
INVALID_EXECUTION_ECONOMICS

3.2. Decision cutoff

Jedna decision unit per birth:

jeden terminalny MFS row
decision_plane = legacy_live

Nie ma:

- wielu anchorów;
- wyboru najlepszego momentu;
- "WAIT" valuation;
- event-by-event firing;
- post-outcome cutoff selection.

Brak terminalnego row:

NON_EVALUABLE_FEATURES: missing_terminal_mfs

Konfliktowe terminalne rows:

INVALID_CAPTURE: conflicting_terminal_mfs

3.3. Feature 1

x1 = MFS.tx_intel_features.dev_volume_ratio

Bez nowego compute.

3.4. Features 2–5

Do MFS zostaje dodana jedna mała grupa:

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AceCoreFeatureProjectionV1 {
    pub status: EvidenceStatus,
    pub new_buyer_intensity_log_ratio: Option<f64>,
    pub new_buyer_quote_flow_sol: Option<f64>,
    pub first_buy_size_late_early_log_ratio: Option<f64>,
    pub first_buy_flow_hhi: Option<f64>,
    pub first_buy_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

Pole MFS:

#[serde(default)]
pub ace_core_features_v1: AceCoreFeatureProjectionV1

Nie zawiera:

- notionalu;
- score’u;
- model output;
- expected PnL;
- candidate;
- action;
- route;
- capacity.

3.5. First-buy cohort

Dla każdego signera wybierany jest pierwszy event spełniający:

successful BUY
przeszedł canonical admission
nie jest duplicate
jest non-dust według istniejącego min_sol_threshold
ma signer identity
ma canonical SOL amount
ma rozstrzygalny canonical order
nie jest późniejszy niż source cutoff

Wykluczone:

- failed;
- sell;
- dust;
- drugi buy tego samego signera;
- post-cutoff event;
- unresolved identity/order.

3.6. Okna

long  = [cutoff - 8000 ms, cutoff]
short = [cutoff - 2000 ms, cutoff]

3.7. Feature 2

N_short = liczba first buyers w ostatnich 2 s
N_long  = liczba first buyers w ostatnich 8 s

lambda_short = (N_short + 1) / 3
lambda_long  = (N_long  + 1) / 9

x2 = ln(lambda_short / lambda_long)

Jest to additive smoothing, nie Bayesian posterior.

3.8. Feature 3

x3 =
    sum(first successful non-dust buy lamports w long window)
    / 1_000_000_000

3.9. Feature 4

Wymagane:

first_buy_count >= 4

Po canonical order:

early = median(first half)
late  = median(second half)

x4 = ln((late + 1) / (early + 1))

Przy nieparzystej liczbie środkowy element trafia do późnej części.

3.10. Feature 5

total = sum first-buy lamports

x5 = Σ(first_buy_lamports_i / total)^2

3.11. Missingness

Cała grupa x2–x5 jest "Unavailable", bez imputacji, gdy:

- cutoff age < 8000 ms;
- retained history jest truncated;
- valid first buyers < 4;
- brakuje signer/SOL amount/order;
- source cutoff jest niedostępny;
- wynik jest non-finite;
- występuje identity/order conflict.

Nie istnieją fallbacki do:

- "unique_signers";
- ogólnego tx rate;
- ogólnego HHI;
- mark momentum;
- average transaction size.

---

4. PR A — brakujący capture

Nazwa PR A

ACE Core: capture cutoff-safe first-buy features and exact execution evidence

PR A nie liczy quote, PnL, model output ani candidate.

4.1. MFS projection

Pliki

ghost-core/src/checkpoint/types.rs
ghost-launcher/src/session/observation.rs

Zmiany

1. Dodać "AceCoreFeatureProjectionV1".
2. Dodać jedno pole do MFS.
3. Dodać pure helper:

fn materialize_ace_core_features_v1(
    &self,
    source_cutoff: &MetricContractDecisionSourceCutoffV1,
) -> AceCoreFeatureProjectionV1

4. Helper czyta wyłącznie:
   - admitted "tx_buffer";
   - existing dust threshold;
   - existing source cutoff;
   - existing retention status.
5. Helper nie odczytuje wall-clock "now", RPC ani globalnego indeksu.
6. Helper nie mutuje session.

4.2. Exact real reserves

Pliki

off-chain/components/seer/src/ipc.rs
ghost-launcher/src/events.rs
ghost-launcher/src/components/seer.rs

Dodać opcjonalne pola

real_sol_reserves: Option<u64>
real_token_reserves: Option<u64>

Do structured relay logu przekazać:

virtual_sol_reserves
virtual_token_reserves
real_sol_reserves
real_token_reserves
complete
slot
write_version
sequence_number
receive/ingress timestamp
account pubkey
account owner
account_data_hash
replay origin

Granica authority

Nowe pola:

- nie zmieniają "AccountStateCore";
- nie zmieniają Gatekeepera;
- nie są alternatywnym runtime price source;
- są używane wyłącznie przez offline replay.

4.3. Jedna aktywna ścieżka execution evidence

PR A nie tworzy równoległych attempt schema.

Implementator musi prześledzić capture config:

config
→ launcher startup
→ Trigger shadow dispatch
→ post-buy shadow lifecycle
→ faktycznie emitowane entry/exit records

Następnie rozszerza wyłącznie:

1. jeden rzeczywiście emitowany entry-attempt/dispatch record;
2. jeden rzeczywiście emitowany exit-attempt/fill record;
3. wspólny sender/confirmation record, jeżeli jest używany przez corpus.

Obecne "PostBuyRuntime" importuje "ShadowEntryAttemptV2" i pozostałe Shadow V2 records, ale plik może zostać zmieniony tylko w zakresie recordów faktycznie emitowanych przez capture profile.

Nie wolno dodawać tych samych pól do kilku nieaktywnych schematów „na zapas”.

4.4. Minimalne pola attempt evidence

Do istniejących aktywnych records dodać addytywnie:

attempt_ordinal: Option<u16>
side: Option<String>                        # buy / sell
route_variant: Option<String>
transaction_signature: Option<String>

intent_ts_ms: Option<u64>
submit_started_ts_ms: Option<u64>
submit_accepted_ts_ms: Option<u64>
confirmation_terminal_ts_ms: Option<u64>
landed_slot: Option<u64>

transaction_size_bytes: Option<u32>
writable_account_count: Option<u16>
account_lock_count: Option<u16>
compute_unit_limit: Option<u32>
compute_unit_price_micro_lamports: Option<u64>
ata_create_class: Option<String>
tip_mode: Option<String>

terminal_execution_class: Option<String>
failure_scope: Option<String>

charged_transaction_costs: Option<TransactionCosts>
cost_evidence_status: Option<String>

max_program_debit_raw: Option<u64>          # BUY
min_output_raw: Option<u64>                 # SELL

Dozwolone terminal classes:

not_submitted
submit_failed
submitted_unconfirmed
confirmation_timeout
confirmation_transport_failed
confirmed_landed
confirmed_on_chain_failed
unknown

Dozwolone "failure_scope":

route_agnostic_transport
route_specific_program
unknown
not_applicable

Dozwolone cost evidence:

complete
not_charged
partial
unknown

4.5. Komponentowy cost ledger

Nie używać:

costs_charged: bool

jako authority.

"charged_transaction_costs" ma zawierać faktycznie naliczone komponenty:

base_fee_lamports
priority_fee_lamports
jito_tip_lamports
ata_rent_lamports
ata_close_refund_lamports
retry_or_failure_cost_lamports

Semantyka:

Not submitted

cost_evidence_status = not_charged
all charged components = 0

Confirmed landed success lub on-chain failure

base fee i priority fee = charged
tip = zgodnie z rzeczywistą konstrukcją/landing evidence
ATA rent/refund = zgodnie z typed account transition

Timeout albo transport unknown

cost_evidence_status = unknown albo partial

Nie wolno automatycznie przyjąć:

all zero

ani:

all configured maximum

Root positive wymaga kompletnego cost evidence dla całej executed selected path.

4.6. Capture profile

Nowy plik:

configs/rollout/ace-core-0p15-net17-capture.toml

Zmienia tylko:

JSON logging = enabled
run-local output paths
V3 replay payload = enabled
watched_pools_ttl_ms = 210000

Pozostawia:

execution_mode = shadow
entry_mode = shadow_only
runtime notional bez zmian
Gatekeeper authority bez zmian
Position Manager authority bez zmian
HET-PM V2 mode bez zmian

4.7. Capture integrity

Podniesienie TTL z 120 s do 210 s zwiększa liczbę równocześnie obserwowanych pooli.

Capture run jest "INVALID_CAPTURE", gdy wystąpi co najmniej jedno:

watched-pool cap eviction
pool removed before required horizon
account-update channel drop
broadcast lag/drop
Seer source stall bez pełnego backfillu
unresolved stream epoch change
conflicting account state order
missing exact reserves w wymaganym intervale
writer drop dla MFS/account/attempt records

Nie powstaje nowy monitoring system. Reuse istniejących:

- log markers;
- writer counters;
- Seer stall/circuit-breaker evidence;
- channel lag/drop counters.

Capture output musi zawierać jeden run-level integrity result:

CAPTURE_VALID
CAPTURE_INVALID

Bez dodatkowej ceremonii ani dashboardu.

4.8. PR A nie zmienia

- Gatekeeper logic;
- Trigger decision logic;
- Position Manager policy;
- V1 thresholds;
- HET-PM V2 mode;
- route builders;
- sender behavior;
- runtime position size;
- runtime slippage;
- PumpSwap;
- process topology.

---

5. Zamrożony execution contract

Nowy input:

configs/ace_core_execution_contract_v1.json

Jego hash jest zamrożony przed outcome reconstruction i model fitting.

5.1. Główne wartości

contract_id = ace_core_0p15_net17_v1

entry_total_wallet_debit_cap_lamports = 150_000_000

entry_route = BuyV2
exit_route  = LegacySell

entry_instruction_protection_bps = 150
safety_exit_instruction_protection_bps = 150

entry_self_impact_max_bps = 200
initial_immediate_exit_impact_max_bps = 200
initial_reserve_displacement_max_bps = 200
position_to_first_buyer_flow_max_bps = 500

net_profit_floor_bps = 1700

policy_tick_ms = 500
hard_loss_fraction = 0.50
inactivity_ms = 30_000
absolute_max_hold_ms = 120_000
crash_guard_mode = observe_only
exit_recovery_ms = 5_000
exit_retry_interval_ms = 500

entry_max_attempts = 3
max_concurrent_positions = 1

max_execution_episode_age_ms = 3_600_000

migration_contract =
    pre_migration_only_pumpswap_unsupported

counterfactual_class =
    observed_path_small_trader_non_propagated

5.2. All-in cap

Dla każdego entry attemptu:

failed_entry_costs_before_k =
    suma faktycznie naliczonych kosztów wcześniejszych attempts

entry_tx_cost_cap_k =
    konserwatywny pełny koszt envelope aktualnego attemptu

remaining_program_budget_k =
    150_000_000
    - failed_entry_costs_before_k
    - entry_tx_cost_cap_k

Binary search wybiera maksymalną ilość tokenów "Q_k", dla której:

protected_program_debit(Q_k)
+ entry_tx_cost_cap_k
+ failed_entry_costs_before_k
<= 150_000_000

protected_program_debit(Q) =
    ceil(trigger_program_debit(Q) × 1.015)

Do "BuyV2":

amount       = Q_k
max_sol_cost = protected_program_debit(Q_k)

"max_sol_cost" nie zawiera transaction costs.

5.3. Fee schedules

Execution contract zawiera slot-resolved records:

route_variant
fee_schedule_id
effective_slot
observed_slot
evidence_kind
evidence_hash
rules

Dla każdego użytego state:

effective_slot <= state.slot
observed_slot <= state.slot

Zakazane:

- future schedule;
- fixture-only schedule jako runtime authority;
- hidden default;
- historyczny stały 1%;
- użycie bieżącego fee schedule wstecz bez slot evidence.

Brak schedule:

INVALID_ECONOMICS

---

6. Initial feasibility dla 0,15 SOL

Initial feasibility jest liczona przed model inference.

6.1. Wymagane warunki

terminal MFS valid
all five features finite
hard risk clean
exact four-reserve state available
fee schedule available
active Pump curve
typed BuyV2 quote available
all-in cap respected
entry self-impact <= 2%
entry reserve displacement <= 2%
position-to-first-buyer-flow <= 5%
immediate full-position LegacySell available na post-entry state
immediate full-position exit impact <= 2%
immediate exit reserve displacement <= 2%

6.2. Znaczenie immediate exit check

Immediate full-position exit check służy wyłącznie do ustalenia:

- czy pozycja 0,15 SOL jest dostatecznie mała przy entry;
- czy observed-path non-propagated approximation jest dopuszczalna;
- czy pozycja miała podstawową odwracalność w momencie wejścia.

Nie jest to późniejszy sell veto.

6.3. Brak rescue mniejszym notionalem

Jeżeli pełny cap 0,15 SOL nie przechodzi initial impact/capacity:

NON_EVALUABLE_CAPACITY

Nie wolno:

- liczyć ponownie dla 0,1 SOL;
- liczyć dla 0,004 SOL;
- dobierać „maksymalnej wygodnej pozycji”;
- skalować mikro-outcome;
- traktować jako zero PnL.

Późniejsza redukcja token quantity jest dozwolona wyłącznie wtedy, gdy rzeczywiście naliczone failed entry costs zmniejszyły pozostały all-in budget.

---

7. Route-agnostic transport episodes

7.1. Cel

Existing DirectBuilder episode nie może stanowić route-specific evidence dla typed "BuyV2/LegacySell".

Może dostarczyć wyłącznie:

submit behavior
transport outcome
confirmation outcome
landing latency
network fee/tip behavior

7.2. Co jest rozstrzygane przez episode

Episode może narzucić:

not submitted
submit failed
confirmation timeout
confirmation transport failed
confirmed landed
landing latency
landed slot
faktycznie naliczone transportowe koszty

7.3. Co jest zawsze rozstrzygane ponownie przez typed ACE replay

program account validity
route activity
max_sol_cost
min_sol_output
program success/failure
program debit/credit
reserve transition
program fees
ATA requirement wynikające z typed account state

Nie wolno przenieść:

DirectBuilder on-chain program failure

jako:

typed BuyV2/LegacySell failure

7.4. Kompatybilność envelope

Episode jest kompatybilny wyłącznie przy zgodności:

side: BUY albo SELL
sender/transport class
transaction-size bucket
writable-account-count bucket
account-lock-count bucket
compute-unit-limit bucket
priority-fee policy class
tip policy class
ATA-create class
region/provider class

Route name nie jest warunkiem zgodności transportu.

7.5. Temporalność

Dla attemptu o czasie "t" można wykorzystać wyłącznie episode:

episode.terminal_ts_ms < t

oraz:

t - episode.terminal_ts_ms <= 3_600_000

Zakazane:

- future episode;
- episode z późniejszego dnia;
- hash selection z całego splitu;
- losowanie;
- wybór najkorzystniejszego episode;
- przenoszenie execution regime z testu do trainu albo odwrotnie.

7.6. Deterministyczny wybór

Spośród wcześniejszych kompatybilnych episodes wybierany jest:

najnowszy wcześniejszy episode

Tie-break:

terminal_ts_ms
→ landed_slot
→ signature/id lexicographic

Każdy retry wybiera episode ponownie dla własnego intent time.

Episode może zostać użyty więcej niż raz, ale output zapisuje:

source_episode_id
source_episode_age_ms
episode_reuse_count

Day-block uncertainty zachowuje zależność wynikającą ze wspólnego execution regime.

7.7. Niekompatybilne failures

Episodes z:

failure_scope = route_specific_program
failure_scope = unknown
cost_evidence_status = partial/unknown

nie są używane do positive-claim execution reconstruction.

Brak wcześniejszego kompletnego kompatybilnego episode:

NON_EVALUABLE_EXECUTION

Jeżeli brak dotyczy attemptu, który zostałby wykonany w sequential selected albo baseline path:

INVALID_EXECUTION_ECONOMICS

i positive root verdict jest niemożliwy.

---

8. Entry replay

8.1. Trigger state

Dla terminalnego decision cutoffu:

1. dodać zamrożony decision-to-submit preparation offset wynikający z execution contract;
2. znaleźć pierwszy exact curve state dostępny nie wcześniej niż intent boundary;
3. sprawdzić max quote age;
4. policzyć typed "BuyV2";
5. wyznaczyć "Q_1" i "max_sol_cost".

8.2. Transport episode

Dla attemptu:

1. wybrać najnowszy wcześniejszy kompatybilny transport episode;
2. zastosować jego submit/confirmation class;
3. nie dziedziczyć route-specific program result.

8.3. Landing state

Dla "confirmed_landed":

1. wyznaczyć landing time/slot z episode;
2. pobrać exact curve state;
3. ponownie policzyć typed "BuyV2" dla tego samego "Q_k";
4. sprawdzić:
   - curve nadal aktywna;
   - landing program debit "<= max_sol_cost";
   - fee schedule valid;
   - account state coherent.

8.4. Entry classes

ENTRY_FILLED

Tylko gdy:

transport confirmed
typed program settlement successful
landing debit <= max_sol_cost
entry total debit <= 150_000_000

ENTRY_FAILED

Znany failure:

submit failure
typed min/max limit failure
known on-chain typed failure
migration przed landing

Koszty naliczane komponentowo.

ENTRY_UNKNOWN

confirmation timeout
confirmation transport uncertainty
ambiguous signature status
unknown cost evidence

"ENTRY_UNKNOWN":

- nie jest sukcesem;
- nie jest known no-position;
- unieważnia dalszy sequential path od tego punktu;
- nie zwalnia slotu w sposób pozwalający na positive claim.

8.5. Retries

Maksymalnie trzy entry attempts.

Po known failed attempt:

1. doliczyć faktycznie charged costs;
2. zaktualizować remaining all-in cap;
3. zastosować istniejące priority/tip increments;
4. wybrać nowy trigger state;
5. wybrać nowy wcześniejszy transport episode;
6. wykonać nowy binary search.

Po unknown attempt:

brak concurrent retry
sequence invalid from unknown

8.6. Final entry accounting

entry_total_debit =
    successful typed program debit
  + successful charged transaction costs
  + wszystkie wcześniejsze charged failed-entry costs

Invariant:

entry_total_debit <= 150_000_000

---

9. Safety lifecycle i net-17 profit gate

9.1. Policy

To nie jest nowy Position Manager.

Offline replay używa:

V1 HardLoss
V1 Inactivity
V1 AbsoluteMaxHold
V1 sticky proposal/recovery semantics

Wyłącza:

V1 mark-based TakeProfit

Dodaje jedną lokalną pure funkcję:

ExecutableNetProfit17

9.2. Frozen policy

tick interval                500 ms
HardLoss                     mark return <= -50%
ExecutableNetProfit17        landed net return >= +17%
Inactivity                   30 000 ms
AbsoluteMaxHold              120 000 ms
CrashGuard                   observe_only
quote recovery               5 000 ms
retry interval               500 ms
full-position only           100% remaining quantity

Reason hierarchy:

1. integrity/route
2. HardLoss
3. ExecutableNetProfit17
4. Inactivity
5. AbsoluteMaxHold
6. Hold

Po utworzeniu proposal reason jest sticky.

9.3. Profit required program credit

Przed SELL attemptem "j":

failed_exit_costs_before_j =
    suma faktycznie charged kosztów wcześniejszych exit attempts

exit_tx_cost_cap_j =
    konserwatywny pełny koszt bieżącego exit envelope

Minimalny program credit:

required_program_credit_j =
    ceil(entry_total_debit × 1.17)
  + failed_exit_costs_before_j
  + exit_tx_cost_cap_j

Do "LegacySell":

amount         = pełna remaining quantity
min_sol_output = required_program_credit_j

9.4. Trigger quote

Profit proposal powstaje tylko, gdy trigger-state typed quote pokazuje:

trigger_program_credit >= required_program_credit_j
route supported
state fresh/coherent
full quantity available

Trigger quote nie jest fill.

9.5. Landing

Dla confirmed transport episode:

1. pobrać landing-state exact reserves;
2. ponownie policzyć typed "LegacySell";
3. sprawdzić "min_sol_output".

Jeżeli:

landed_program_credit < min_sol_output

wynik:

EXIT_FAILED
reason = min_output_not_met_at_landing

Pozycja pozostaje otwarta. Attempt cost jest doliczony.

9.6. Successful profit invariant

Każdy:

EXIT_FILLED
reason = ExecutableNetProfit17

musi spełnić:

exit_net_credit =
    landed program credit
  - current charged exit costs
  - earlier charged failed-exit costs

net_pnl = exit_net_credit - entry_total_debit
net_return = net_pnl / entry_total_debit

net_return >= 0.17

9.7. Safety exit protection

HardLoss, Inactivity i AbsoluteMaxHold nie wymagają +17%.

Ich instruction protection:

safety_min_sol_output =
    floor(trigger_program_credit × 0.985)

Landing credit poniżej minimum powoduje failure, nie ukryty gorszy fill.

9.8. Późniejszy exit impact

Po successful entry późniejszy full-position exit impact:

- jest liczony;
- jest logowany;
- wpływa bezpośrednio na typed program credit;
- wpływa na net PnL;
- nie stanowi osobnego veto.

Nie stosować później:

exit impact <= 2%
reserve displacement <= 2%

jako warunku SELL.

Limity 2% obowiązują wyłącznie w initial feasibility.

Przykład poprawny:

later full-exit impact = 3%
landed net return = +18%
→ EXIT_FILLED

9.9. Exit retries

Po known failure:

attempt w t=0
kolejne attempts co 500 ms
do elapsed < 5000 ms

Każdy attempt:

- ma własny wcześniejszy transport episode;
- ma nowy landing state;
- ma własny cost ledger;
- zwiększa required profit credit o charged failure costs.

Po unknown exit execution:

EXIT_UNKNOWN
sequence invalid from unknown
brak concurrent resend

---

10. Migration i POSITION_STUCK

10.1. Contract

pre_migration_only_pumpswap_unsupported

Nie dodajemy PumpSwap.

10.2. Migration przed entry fill

ENTRY_FAILED
reason = route_migrated_before_entry_fill

Pozycja nie powstaje.

Koszty zgodnie z rzeczywistym execution class.

10.3. Migration po entry

Po "complete=true":

- pozycja pozostaje w denominatorze;
- PumpSwap mark/value nie jest używany;
- LegacySell jest unsupported;
- brak confirmed full-position fill;
- po bounded recovery pozycja przechodzi do "POSITION_STUCK".

10.4. Inne źródła POSITION_STUCK

EXIT_RECOVERY_EXHAUSTED
unsupported route po entry
known permanent route failure
brak full-position fill po max recovery

10.5. Ekonomika POSITION_STUCK

Przy przejściu do stuck:

economic_recovery_value = 0
economic_net_pnl =
    -entry_total_debit
    - wszystkie charged exit-failure costs

"economic_terminal_ts" jest chwilą rozpoznania stuck.

Jednocześnie:

position quantity pozostaje niesprzedana
capacity pozostaje OCCUPIED

10.6. Capacity POSITION_STUCK

"POSITION_STUCK" nie zwalnia slotu.

occupancy_end_ts = end untouched test period

Po pierwszej stuck position:

wszystkie późniejsze candidates = SKIPPED_CAPACITY

Nie wolno utożsamiać:

konserwatywna wartość ekonomiczna zero

z:

potwierdzone zamknięcie pozycji

10.7. Unknown po entry

"UNKNOWN_EXECUTION" po otwarciu pozycji nie daje nawet pewności, czy SELL nastąpił.

W takim przypadku:

sequential_path_status = INVALID_FROM_UNKNOWN_EXECUTION

Od tej chwili:

- nie wylicza się positive sequential claim;
- nie zakłada się zwolnienia capacity;
- root positive jest niemożliwy;
- subtype = invalid_execution_economics.

---

11. Sequential max-one-position replay

11.1. Ordering

Candidates są sortowane:

cutoff_ts_ms
→ decision_slot, null last
→ source event order
→ candidate_id lexicographic

11.2. State

FREE
ENTRY_PENDING
OCCUPIED
POSITION_STUCK
INVALID_UNKNOWN

11.3. Transitions

FREE → ENTRY_PENDING

Pierwszy chronologiczny model candidate.

ENTRY_PENDING → FREE

Tylko po terminalnym known "ENTRY_FAILED", gdy wiadomo, że pozycja nie powstała.

ENTRY_PENDING → OCCUPIED

Po "ENTRY_FILLED".

ENTRY_PENDING → INVALID_UNKNOWN

Po "ENTRY_UNKNOWN".

OCCUPIED → FREE

Wyłącznie po confirmed:

EXIT_FILLED
quantity = pełna remaining quantity

OCCUPIED → POSITION_STUCK

Po known recovery exhaustion/unsupported route bez SELL fill.

OCCUPIED → INVALID_UNKNOWN

Po "EXIT_UNKNOWN".

POSITION_STUCK

Pozostaje do końca testu.

INVALID_UNKNOWN

Dalsza sekwencja nie może wspierać positive verdict.

11.4. Overlapping candidates

W stanach:

ENTRY_PENDING
OCCUPIED
POSITION_STUCK

kolejne candidates:

SKIPPED_CAPACITY

Nie wpływają na PnL ani coefficients.

11.5. Stały kapitał

Każda nowa pozycja używa all-in capu 0,15 SOL.

Brak:

- compounding;
- zmiany notionalu po zysku/stracie;
- dynamicznego wallet modelu;
- wielu slotów.

11.6. Baseline

"all_evaluable_sequential_baseline" używa identycznych:

- feasibility gates;
- notionalu;
- costs;
- execution episodes;
- policy;
- ordering;
- capacity;
- stuck semantics.

Różnica:

baseline otwiera pierwszy EVALUABLE record przy FREE
model lane otwiera pierwszy EVALUABLE + MODEL_POSITIVE record przy FREE

---

12. Model

12.1. Target

raw executable net PnL lamports

Target zawiera:

- entry failures;
- successful entries;
- exit failures;
- retry costs;
- migration losses;
- stuck zero-recovery value;
- successful profit/safety exits.

Rows z unknown execution nie są prawidłowym targetem i powodują invalid execution economics na ścieżce claimu.

12.2. Model

Jedna fixed rodzina:

Ridge regression
fit_intercept = true
alpha = 1.0
loss = squared error
features = dokładnie x1..x5

Brak:

- model zoo;
- GBDT;
- interaction terms;
- polynomial terms;
- feature selection;
- hyperparameter sweep;
- target clipping;
- winsorization;
- refit po test period.

12.3. Scaling

Fit wyłącznie na train:

median
IQR

Jeżeli "IQR = 0" dla którejkolwiek cechy:

model invalid

Nie usuwać cechy i nie dodawać fallbacku.

12.4. Split

Chronologicznie po pełnych UTC days:

60% najwcześniejszych dni → train
20% kolejnych             → calibration
20% najpóźniejszych       → untouched test

Żaden dzień nie przecina splitów.

12.5. Candidate rule

Po fitowaniu na train:

predicted_expected_net_pnl_lamports > 0

Calibration tail gate:

selected calibration CVaR20
>= all-evaluable sequential calibration baseline CVaR20

Jeżeli calibration tail gate nie przejdzie:

positive root verdict niemożliwy

Nie dobieramy progu expected PnL pod maksymalizację calibration profit.

---

13. Standard statystyczny

13.1. Minimalny untouched floor

Positive verdict wymaga:

co najmniej 20 niezależnych UTC calendar-day blocks
co najmniej 50 otwartych model-selected positions
z ENTRY_FILLED i terminalnym known economic outcome
po sequential capacity filtering

Failed entries:

- są w PnL;
- nie liczą się do 50 otwartych pozycji.

POSITION_STUCK:

- jest known economic outcome;
- liczy się jako otwarta pozycja;
- blokuje późniejszą capacity.

Unknown execution:

- nie liczy się jako terminal known;
- unieważnia positive claim.

13.2. Opportunity floor

Wymagane:

średnio co najmniej 1 otwarta selected position
na każdy untouched UTC day

Nie ma arbitralnego progu procentowego względem całego birth universe.

Feasibility rate względem universe jest raportowany, ale nie stanowi osobnego PASS/FAIL.

13.3. Bootstrap

Dwa one-sided lower 95% day-block bounds:

1. mean net PnL per opened selected position;
2. sequential daily net PnL, z zero-trade days.

10 000 resamples całych UTC day blocks
fixed seed
wszystkie rows dnia pozostają razem

13.4. Tail

Na untouched test:

selected sequential CVaR20
all-evaluable sequential baseline CVaR20

Positive wymaga:

selected CVaR20 >= baseline CVaR20

13.5. Stuck position i statystyka

Jeżeli pozycja przechodzi do "POSITION_STUCK":

- strata jest przypisana do dnia "economic_terminal_ts";
- capacity pozostaje occupied przez kolejne dni;
- kolejne dni mają zero nowych trades;
- zero-trade days pozostają w daily bootstrap;
- nie wolno skrócić test periodu na dni przed stuck.

---

14. PR B — implementacja

Nazwa PR B

ACE Core: replay 0.15 SOL all-in and net-17 full-position exits

14.1. Rust binary

Nowy plik:

ghost-launcher/src/bin/ace_core_replay.rs

Input:

--birth-events
--decision-jsonl
--account-update-jsonl
--execution-episode-jsonl
--execution-contract
--assessment-out
--outcome-out

Etapy:

1. full universe;
2. terminal MFS join/hash;
3. x1–x5 extraction;
4. exact curve timeline;
5. fee schedule resolution;
6. initial 0,15 feasibility;
7. time-local execution episode selection;
8. entry attempts;
9. safety/net17 lifecycle;
10. exit attempts;
11. migration/stuck handling;
12. deterministic terminal rows.

14.2. V1 façade

Jeżeli visibility tego wymaga, w:

ghost-brain/src/guardian/post_buy/exit_policy_v1.rs

dodać minimalną crate-public pure façade zwracającą:

HardLoss
Inactivity
AbsoluteMaxHold
Hold

Nie zmieniać obecnego evaluator call site.

Nie umieszczać net17 w module PM. Net17 pozostaje lokalną pure funkcją offline replay.

14.3. Python evaluator

Nowy plik:

scripts/ace_core_ev_test.py

Odpowiada za:

- chronological split;
- scaler;
- Ridge fit;
- calibration tail gate;
- model predictions;
- sequential selected replay;
- sequential baseline;
- day-block bootstrap;
- root verdict.

Rust binary odpowiada za ekonomiczny outcome. Python nie przelicza quote ani PnL.

---

15. Output contracts

15.1. Assessment row

Jeden row per birth:

schema
candidate identity
birth order/time
terminal cutoff/order
MFS hash/version
x1..x5
feature availability
hard-risk status
exact reserve coverage
fee schedule status
initial Q for 0,15 cap
program debit
transaction cost cap
total debit cap
entry impact
reserve displacement
position-to-flow
immediate exit impact/capacity
feasibility status
exact reason

15.2. Outcome row

Jeden row per attempted candidate:

candidate identity
execution contract hash
counterfactual class

entry attempts[]
  source episode
  source episode age
  transport class
  typed landing result
  component costs

entry terminal class
entry_total_debit
entry quantity
entry state

exit attempts[]
  reason
  source episode
  source episode age
  typed landing result
  component costs

exit terminal class
economic terminal class
position state:
  closed
  stuck
  unknown

economic terminal timestamp
occupancy end timestamp

program credit
charged costs
net PnL
net return
migration status
later exit impact

model prediction
model candidate
sequential status

15.3. Root result

ace_core_ev_result_v1.json

Zawiera:

- input hashes;
- capture integrity;
- execution contract;
- model coefficients;
- split days;
- counts/reasons;
- episode age/reuse;
- cost coverage;
- feasibility/capacity;
- entry/exit failures;
- unknowns;
- stuck positions;
- migration;
- per-trade metrics;
- sequential metrics;
- bootstrap bounds;
- baseline comparison;
- root verdict/subtype.

---

16. Focused tests

PR A

A1. Feature projection parity

Ten sam admitted tx set daje identyczne x2–x5.

A2. Duplicate exclusion

Duplicate nie zmienia cohortu.

A3. Failed/sell/dust exclusion

Nie tworzą first buyer.

A4. Cutoff

Post-cutoff tx nie zmienia cech.

A5. Missingness

Brak danych daje "Unavailable", nie zero.

A6. Decision parity

Gatekeeper/Trigger/PM outputs pozostają identyczne.

A7. Exact reserves

Raw account bytes → parser → IPC → launcher log zachowuje bit-identical four reserves.

A8. Reducer non-authority

Nowe real reserves nie zmieniają AccountStateCore/Gatekeeper.

A9. Component costs

Każdy koszt ma osobne evidence; boolean nie jest authority.

A10. Failure scope

Route-specific i transport failures są rozróżnione.

A11. Capture invalidation

Eviction/drop/stall daje "CAPTURE_INVALID".

PR B — entry

B1. All-in cap

entry_total_debit <= 150_000_000

B2. Costs reduce Q

Większe koszty zmniejszają program budget i quantity.

B3. No 0,004 fallback

Micro notional nie występuje w replay.

B4. Landing max_sol_cost

Trigger success + landing debit ponad cap = failed program attempt.

B5. Route-specific episode isolation

DirectBuilder program failure nie powoduje automatycznie typed failure.

B6. Future episode forbidden

Episode późniejszy od attemptu nigdy nie jest używany.

B7. Lookback

Episode starszy niż 60 min jest niedopuszczalny.

B8. Latest-prior determinism

Ten sam corpus daje ten sam episode.

PR B — net17

B9. Mark +17, net <17

Brak profit proposal/fill.

B10. Gross +17, net <17

Brak profit fill.

B11. Trigger +17, landing poniżej min output

"EXIT_FAILED".

B12. Retry cost raises floor

Required program credit rośnie o charged failure costs.

B13. Successful net17

Każdy profit fill ma actual net return ">=17%".

B14. Full quantity

Partial sell niedopuszczalny.

PR B — impact

B15. Runtime 25% ignored

Zmiana config slippage nie zmienia ACE.

B16. Initial impact gate

Initial impact >2% daje non-evaluable capacity.

B17. No smaller rescue

Niewykonalny record nie jest liczony mniejszym notionalem.

B18. Later impact no veto

Later impact >2%, lecz net17 spełnione → successful profit fill.

B19. Later safety impact

Safety exit nie jest blokowany samym przekroczeniem 2%.

PR B — migration/stuck

B20. Migration remains denominator

Post-entry migration nie usuwa row.

B21. Unsupported migration becomes stuck

Brak fill → POSITION_STUCK.

B22. Stuck does not free capacity

Późniejsze candidates są skipped do końca testu.

B23. Recovery exhausted becomes stuck

Nie jest traktowane jak confirmed close.

B24. Unknown invalidates sequence

Unknown po entry nie zwalnia capacity i blokuje positive claim.

PR B — sequential

B25. Only confirmed exit frees slot

Jedyny poprawny OCCUPIED→FREE to full "EXIT_FILLED".

B26. No overlap

Nie ma dwóch pozycji równocześnie.

B27. Tie-break determinism

Identyczny cutoff daje stabilną kolejność.

B28. Baseline parity

Baseline i model lane różnią się wyłącznie selection rule.

PR B — model/statystyka

B29. Exactly five features

Model matrix ma pięć kolumn.

B30. Raw PnL target

Target nie jest label/quantile/mark proxy.

B31. Chronological isolation

Test days nie uczestniczą w fit/calibration.

B32. Floor

19 days albo 49 terminal positions → insufficient data.

B33. Bootstrap determinism

Ten sam seed daje te same bounds.

B34. Daily path includes stuck days

Dni po stuck pozostają zero-trade days.

B35. Authority absence

ACE binary/script nie jest importowany przez runtime.

---

17. Root verdict

17.1. Positive

Jedyny positive verdict:

ACE_CORE_0P15_NET17_SHOWS_POSITIVE_EXECUTABLE_EV

Wymaga jednocześnie:

1. "CAPTURE_VALID";
2. complete five-feature evidence;
3. complete slot-resolved fee schedules;
4. complete cost evidence na executed selected path;
5. wyłącznie wcześniejsze, time-local execution episodes;
6. brak route-specific failure inheritance;
7. all-in cap 0,15 zawsze zachowany;
8. brak mikro-fallbacku;
9. minimum 17% net dla każdego profit fill;
10. wszystkie known failed attempt costs policzone;
11. migration pozostała w denominatorze;
12. POSITION_STUCK nie zwolniło capacity;
13. brak unknown execution na selected sequential claim path;
14. co najmniej 20 untouched UTC days;
15. co najmniej 50 opened terminal selected positions;
16. średnio co najmniej jedna otwarta selected position na untouched day;
17. lower 95% day-block bound mean selected PnL > 0;
18. sequential total PnL > 0;
19. lower 95% day-block bound sequential daily PnL > 0;
20. selected CVaR20 nie gorszy od honest baseline;
21. calibration tail gate przeszedł;
22. runtime authority bez zmian;
23. deterministic replay.

17.2. Negative lub nieważny

Każdy inny wynik:

ACE_CORE_0P15_NET17_DOES_NOT_SHOW_POSITIVE_EXECUTABLE_EV

Subtype:

negative
inconclusive
insufficient_data
invalid_capture
invalid_economics
invalid_execution_economics
unsupported_route
insufficient_capacity

Precedence:

invalid_capture
→ invalid_economics
→ invalid_execution_economics
→ unsupported_route
→ insufficient_capacity
→ insufficient_data
→ negative / inconclusive

POSITION_STUCK wynikający z faktycznie wybranego pre-migration-only contractu jest zwykłym economic loss, a nie automatycznie "unsupported_route".

---

18. Finalny zakres plików

PR A

ghost-core/src/checkpoint/types.rs
ghost-launcher/src/session/observation.rs

off-chain/components/seer/src/ipc.rs
ghost-launcher/src/events.rs
ghost-launcher/src/components/seer.rs

ghost-launcher/src/components/trigger/shadow_run.rs
  tylko aktywny entry/dispatch record

ghost-brain/src/guardian/post_buy/shadow_v2.rs
lub aktywny istniejący post-buy attempt/fill record
  tylko gdy capture profile rzeczywiście go emituje

ghost-launcher/src/components/live_tx_sender.rs
  wyłącznie minimalne component/transport evidence,
  jeśli istniejący sender record jest źródłem corpus

configs/rollout/ace-core-0p15-net17-capture.toml

focused tests

Nie wolno modyfikować kilku równoległych inactive attempt schemas.

PR B

ghost-launcher/src/bin/ace_core_replay.rs
ghost-launcher/Cargo.toml

ghost-brain/src/guardian/post_buy/exit_policy_v1.rs
  tylko minimalna safety-only visibility façade, jeśli konieczna

configs/ace_core_execution_contract_v1.json
scripts/ace_core_ev_test.py

focused fixtures/tests

Brak trzeciego PR-a.

---

19. Kolejność rozpoczęcia prac

PR A

1. Potwierdzić HEAD.
2. Prześledzić aktywne capture record call sites.
3. Dodać MFS projection.
4. Dodać exact real reserves.
5. Dodać minimalne component cost/transport evidence tylko do aktywnych records.
6. Dodać capture profile.
7. Dodać focused tests.
8. Uruchomić capture.
9. Sprawdzić "CAPTURE_VALID".

PR B

1. Dodać execution contract validator.
2. Dodać exact curve/fee replay.
3. Dodać initial 0,15 feasibility.
4. Dodać latest-prior transport episode matching.
5. Dodać entry attempts.
6. Dodać V1 safety + net17.
7. Dodać exit attempts.
8. Dodać migration/stuck semantics.
9. Dodać per-birth outcomes.
10. Dodać frozen Ridge model.
11. Dodać sequential selected/baseline replay.
12. Dodać day-block uncertainty.
13. Zapisać root verdict.
14. STOP.

---

20. Definicja ukończenia

ACE Core jest ukończony, gdy jedna zamrożona procedura tworzy:

full birth denominator
+ dokładnie pięć cutoff-safe cech
+ initial feasibility dla all-in 0,15 SOL
+ typed BuyV2 trigger/landing
+ prior time-local transport evidence
+ component-wise entry costs
+ V1 safety lifecycle
+ landed executable-net +17% gate
+ typed LegacySell trigger/landing
+ component-wise exit/failure costs
+ migration/POSITION_STUCK handling
+ max-one-position sequential path
+ frozen conditional-mean model
+ 20-day untouched test
+ jeden root verdict

Końcowy output jest wyłącznie jednym z:

ACE_CORE_0P15_NET17_SHOWS_POSITIVE_EXECUTABLE_EV

albo:

ACE_CORE_0P15_NET17_DOES_NOT_SHOW_POSITIVE_EXECUTABLE_EV

Nie istnieje automatyczny następny etap.
