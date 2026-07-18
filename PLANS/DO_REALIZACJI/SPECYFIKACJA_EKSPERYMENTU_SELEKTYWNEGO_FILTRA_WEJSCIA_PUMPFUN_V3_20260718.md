# Specyfikacja eksperymentu: selektywny filtr wejścia pump.fun / PumpSwap V3

## 0. Status, rola normatywna i granica uprawnień

Status:

```text
EXPERIMENT_SPEC_T0_ARCHITECTURE_PASS
TYPE5_SELECTOR_ABLATION_ONLY
NO_SECOND_SCORING_STACK
EXECUTABLE_NET_RETURN_PRIMARY_TARGET
FULL_MARKET_UNIVERSE_REQUIRED
DECISION_TIME_CUTOFF_REQUIRED
HISTORICAL_DISCOVERY_NOT_PROMOTION_EVIDENCE
UNTOUCHED_HOLDOUT_REQUIRED
PROSPECTIVE_SHADOW_CONFIRMATION_REQUIRED
NO_RUNTIME_CHANGE
NO_BUY_REJECT_TIMEOUT_CHANGE
NO_POSITION_SIZING_CHANGE
IMPLEMENTATION_PLAN_REQUIRED_SEPARATELY
POLICY_PROMOTION_REQUIRED_SEPARATELY
```

Data: `2026-07-18`

Repozytorium: `smahacfel/Gho`

Baseline dokumentu: `origin/main` na commit `6962d79e8369d4f89d7865f824b91146d0eff99e`.

Rola dokumentu:

> Ten dokument jest normatywną specyfikacją eksperymentu i wejściem do osobnego planu wykonawczego. Nie jest planem implementacji, nie zatwierdza zmian Rust/TOML, nie uruchamia zbierania danych, nie stroi Gatekeepera i nie daje zgody na zmianę aktywnego lub shadow authority.

Plan wykonawczy przygotowany na podstawie tej specyfikacji musi zachować wszystkie wymagania, granice semantyczne, artefakty, testy falsyfikacyjne i bramki PASS/FAIL opisane poniżej. Może doprecyzować nazwy typów, kolejność PR-ów i budżety techniczne, ale nie może osłabić kontraktów statystycznych, execution-truth ani shadow/live separation bez osobnego amendmentu tej specyfikacji.

## 1. Decyzja nadrzędna

Hipoteza nie tworzy nowego systemu decyzyjnego. Jest sekwencją paired ablations w istniejącym przepływie:

```text
canonical producers
→ MaterializedFeatureSet / cutoff-safe selector evidence
→ Early Flow / Coordination assessment
→ istniejący Gatekeeper V3
→ shadow candidate ranking / assessment
→ executable outcome reconstruction
→ historical discovery
→ frozen candidate
→ untouched out-of-time holdout
→ prospective shadow validation
→ osobny policy-promotion plan
```

Nie powstają nowe odpowiedniki:

- `RiskVerdictStatus`;
- `OpportunityVerdictStatus`;
- `ConfidenceBreakdown`;
- `V3ShadowDecision`;
- finalnego arbitra BUY/REJECT/TIMEOUT;
- drugiego właściciela `MaterializedFeatureSet`;
- drugiego runtime scoring stacku;
- drugiego Position Managera ani drugiej polityki lifecycle authority.

Istniejący Gatekeeper V3 pozostaje jedynym docelowym właścicielem risk, opportunity, confidence, reason chain i shadow verdictu. Do zakończenia wszystkich bramek eksperymentu wynik nowych ablations jest wyłącznie offline/shadow evidence i nie może wpływać na realne ani primary-shadow BUY/REJECT/TIMEOUT.

## 2. Pytanie badawcze

Główne pytanie brzmi:

> Czy dodanie cutoff-safe informacji o jakości i niezależności wczesnego buyer flow do obecnego baseline'u Ghosta zwiększa realizowalny zwrot netto z zaakceptowanych wejść, po uwzględnieniu dokładnego stanu rynku, latencji, opłat, poślizgu, niewykonanych prób i zamrożonej polityki wyjścia?

Dla każdej kolejnej warstwy `B_n` testowana jest hipoteza zerowa:

\[
H_{0,n}: \Delta EV_{net}(B_n, B_{n-1}) \le 0
\]

przeciwko:

\[
H_{1,n}: LCB_{95\%}\left[\Delta EV_{net}(B_n, B_{n-1})\right] > 0.
\]

Porównanie jest paired: oba warianty są oceniane na tym samym universe, tych samych anchorach, tych samych ścieżkach, tej samej polityce wyjścia, tych samych kosztach, tych samych foldach i tym samym modelu latencji.

Samo dodatnie średnie EV, dodatni wynik w jednym tygodniu, wysoka AUC, graduation lift albo lepszy in-sample score nie stanowią PASS.

## 3. Główny target i outcome vector

### 3.1. Główny target

Graduacja nie jest głównym targetem.

Głównym wynikiem dla decyzji wejścia w chwili `T_d`, polityki wyjścia `k` i horyzontu `h` jest:

\[
Y_{k,h}=R_{net}(T_d,k,h,L,F,S,E),
\]

gdzie:

- `L` — rzeczywista lub zamrożona dystrybucja latencji;
- `F` — historycznie właściwy fee schedule i network fees;
- `S` — price impact oraz slippage;
- `E` — execution outcome, w tym brak wejścia, brak wyjścia, retry, stale quote i route transition.

Zwrot netto:

\[
R_{net}=\frac{Q_{sell,out}-Q_{buy,in}-C_{network}-C_{tips}-C_{interface}}{Q_{buy,in}}.
\]

`Q_{buy,in}` i `Q_{sell,out}` pochodzą z exact executable quote albo z potwierdzonego fill evidence. Mark price, market cap, reserve-derived spot price ani MFE mark proxy nie mogą zastąpić executable outcome.

### 3.2. Outcome vector

Dla każdego `(candidate_id, decision_anchor_id, notional_profile_id, latency_profile_id)` zachowywany jest pełny wektor:

```text
R_net dla PRIMARY exit policy
R_net dla każdej zamrożonej sensitivity exit policy
entry_execution_status
exit_execution_status
entry_landing_slot / observed landing time
exit_landing_slot / observed landing time
MFE executable i mark-only — osobno
MAE executable i mark-only — osobno
peak-to-terminal giveback
time_to_first_executable_profit
time_to_liquidity_failure
time_to_curve_complete
time_to_migration
graduated
post-graduation survival auxiliaries
rug/extraction auxiliary labels
```

Nie wolno wybierać exitu per token po zobaczeniu ścieżki. Jedna polityka jest oznaczona jako `PRIMARY`; pozostałe są z góry zamrożonym sensitivity setem.

### 3.3. Pomocnicze etykiety

Dopuszczalne jako auxiliary diagnostics:

- graduacja w zadanym horyzoncie;
- graduacja i utrzymanie wartości;
- MFE/MAE;
- curve completion i migration timing;
- creator/dev sell;
- extraction/rug proxy;
- survival/hazard outcomes;
- social-presence graduation association.

Żadna auxiliary label nie może być prezentowana jako dowód dodatniego trading EV.

## 4. Universe i jednostka analizy

### 4.1. Universe główny

Pierwszy eksperyment obejmuje wyłącznie:

```text
mainnet-beta
Pump program
SOL-paired canonical bonding-curve launches
wszystkie poprawnie rozpoznane create/create_v2-equivalent birth events
bez filtrowania po activity, verdict, volume, social presence, graduacji ani dostępności lifecycle
```

Program Pump:

```text
6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P
```

USDC-paired launches są osobną kohortą i nie mogą być mieszane z SOL w modelu, fee registry, quote semantics ani raportach. Ich rozszerzenie wymaga osobnego cohort ID i osobnej kwalifikacji.

Universe nie może być budowany z:

- accepted BUY rows;
- lifecycle positions;
- tokenów obecnych w outcome logs;
- listy graduowanych tokenów;
- API zwracającego wyłącznie aktywne/popularne tokeny;
- późniejszego discovery po adresach, które osiągnęły wynik.

### 4.2. Source-of-truth universe

Historyczny universe musi powstać z archiwalnych canonical blocks / transakcji lub równoważnego kompletnego indeksu programu Pump. Prospektywny universe powstaje z canonical Yellowstone/Geyser ingest lane.

RPC per-address, explorer API i zewnętrzne indeksy mogą być wyłącznie:

```text
enrichment
flagged backfill
coverage audit
```

Nie mogą po cichu zastąpić denominatora ani canonical event order.

### 4.3. Jednostki

Podstawowa jednostka rynku:

```text
candidate_id = mint:bonding_curve:birth_chain_identity
```

Podstawowa jednostka oceny wejścia:

```text
candidate_decision_unit =
(candidate_id, decision_anchor_id, notional_profile_id, latency_profile_id)
```

Podstawowa jednostka paired comparison:

```text
(candidate_decision_unit, B_n, B_{n-1}, primary_exit_policy_id)
```

Jeden launch nie może być traktowany jako wiele niezależnych obserwacji statystycznych tylko dlatego, że posiada wiele anchorów. Bootstrap, split i effective sample size muszą klastrować po launchu co najmniej na najniższym poziomie.

## 5. Kontrakt identity, event order i czasu

Każdy normalized event zachowuje co najmniej:

```text
candidate_id
mint
bonding_curve
pool_id, jeśli istnieje
creator
create_user
quote_mint
signature
slot
transaction_index
instruction_index
inner_instruction_index
event_ordinal
instruction_variant
success / exact error class
chain_order_key
block_time_s
source_observed_wall_ms, jeśli dostępne
source_observed_monotonic_ns, tylko prospective
source_stream_id
source_schema_version
decoder_version
program_id
program_version_or_idl_hash
```

Canonical chain order:

```text
(slot, transaction_index, instruction_index, inner_instruction_index, event_ordinal)
```

`blockTime` nie jest timestampem milisekundowym. Historyczny proces przybyć nie może tworzyć fikcyjnych sub-second czasów z sekundowego `blockTime`. Historyczne analizy sekwencyjne używają chain order, slotów i jawnie określonego modelu slot-time. Dokładne processing/interarrival timings są kwalifikowane wyłącznie na prospektywnym streamie z lokalnym monotonic clock.

Należy rozdzielić:

```text
T0_chain
T0_observed
chain event time
processing time
wall-clock time
landing time
confirmation time
```

Mieszanie domen czasu bez typed provenance dyskwalifikuje rekord.

Deduplikacja musi być deterministyczna. Signature bez instruction/event ordinal nie wystarcza, jeżeli jedna transakcja zawiera wiele zdarzeń programu. Collision, ambiguous identity albo conflicting payload oznaczają fail-closed i trafiają do manifestu.

## 6. Protocol, decoder i fee registry

Eksperyment musi używać wersjonowanego rejestru:

```text
program_id
IDL / decoder hash
instruction variant set
account schema version
quote algorithm version
fee_schedule_id
fee_schedule_effective_from_slot
effective_to_slot albo open-ended
pool class
quote mint class
```

Decoder musi rozpoznawać odpowiednie dla analizowanego okresu instrukcje, w tym legacy i nowe interfejsy, między innymi:

```text
create / create_v2, jeśli aktywne w okresie
buy
sell
buy_exact_quote_in
buy_v2
sell_v2
buy_exact_quote_in_v2
migrate
canonical PumpSwap swaps
```

Nieznany discriminator, nieznany account layout albo program upgrade w środku runu nie może być mapowany do najbliższego wariantu. Okres zostaje podzielony na schema/protocol regimes albo rekord staje się non-evaluable.

Fee schedule jest historyczny. Aktualna tabela nie może być stosowana wstecz do wcześniejszych slotów bez dowodu zgodności programu. Dla stanu obowiązującego od maja 2026 oficjalna dokumentacja wskazuje między innymi:

```text
bonding curve total fee = 1.25%
canonical PumpSwap = dynamic 1.25% ... 0.30% zależnie od quote-market-cap
non-canonical PumpSwap = 0.30%
```

Te wartości są źródłem konfiguracji rejestru, nie hard-coded skrótem backtestu. Exact quote i fee math mają być odtwarzane z programu/SDK/zwalidowanej lokalnej implementacji na stanie rezerw właściwym dla chwili wykonania.

## 7. Decision anchors i cutoff

### 7.1. Wall-clock anchors

Obowiązkowy frozen grid discovery:

```text
birth+1s
birth+2s
birth+3s
birth+5s
birth+10s
birth+30s
```

Historyczne anchors muszą jawnie raportować ograniczenie timestamp resolution. Prospektywne anchors używają `T0_observed + duration` i monotonic clock.

### 7.2. Event-time anchors

Obowiązkowy grid:

```text
2nd, 3rd, 5th, 8th, 13th newly observed buyer
```

Jest to grid obserwacyjny, nie próg BUY i nie odtworzenie progów `6-12` z v1. Anchor istnieje tylko, gdy zdarzenie było dostępne przed cutoffem i miało clean buyer identity.

### 7.3. State anchors

Obowiązkowy grid:

```text
bonding_progress = 5%, 10%, 20%, 30%, 50%
```

State anchor używa pierwszego canonical account state osiągającego próg. Nie wolno interpolować z późniejszej próbki ani używać post-migration knowledge do naprawy wcześniejszego stanu.

### 7.4. Adaptive stopping

Adaptive stopping jest analizą kandydata dopiero po zbudowaniu pełnego zestawu statycznych anchorów. Nie może cenzurować danych ani kończyć historycznego capture po wczesnym REJECT.

Jeżeli adaptive policy zostanie wybrana, musi mieć:

- deterministic stopping rule;
- typed `STOP_EVIDENCE_SUFFICIENT`, `STOP_HARD_RISK`, `STOP_DEADLINE`;
- maksymalny deadline;
- replay equivalence;
- osobną paired comparison względem najlepszego frozen statycznego anchora;
- brak wglądu w późniejsze outcomes.

## 8. Feature groups i paired ablation ladder

### B0 — baseline

Obecny cutoff-safe baseline Gatekeeper/selector dostępny na freeze commit. Baseline identity obejmuje dokładny commit, config hash, materialization/schema versions i feature list.

### B1 — raw buyer participation

Co najmniej:

```text
raw_unique_buyers
successful_buy_count
new_buyer_rate
first-new-buyer chain-order sequence
buyer increments per anchor
repeat buys per buyer
unique buyer / buy count ratio
```

Buyer oznacza user/wallet wykonujący skuteczną instrukcję buy. Nie oznacza dowolnego signera transakcji. Fallback `unique_signers → unique_buyers` jest zakazany w clean experiment denominator. Może istnieć wyłącznie jako osobna degraded diagnostic feature.

### B2 — buyer-cluster independence

Raw buyer count jest rozszerzony o cutoff-safe clustering na podstawie evidence dostępnego przed decyzją.

Obowiązkowo rozdzielone są:

\[
EIC_{count}=\frac{1}{\sum_c(n_c/N)^2}
\]

oraz:

\[
EIC_{flow}=\frac{1}{\sum_c(v_c/V)^2}.
\]

`EIC_count` opisuje efektywną liczbę klastrów uczestnictwa. `EIC_flow` opisuje efektywną liczbę klastrów kontrolujących buy flow. Żadna z tych miar nie może być nazywana udowodnioną liczbą niezależnych ludzi.

Kontekst obejmuje co najmniej:

```text
funding-source HHI
direct creator/deployer-to-buyer links
same-source cluster sizes
funding shortly before launch
first-N buyer cohort reuse
repeat buyer share
cross-pool velocity
known/unknown funding coverage
cluster algorithm/version/config hash
```

Przy niepełnym funding evidence emitowane są:

```text
EIC_lower: wszystkie unknown należą do jednego klastra
EIC_upper: każdy unknown jest osobnym klastrem
EIC_point: jawnie zdefiniowana estymata, tylko jeśli dopuszczona przez quality contract
```

Szeroki przedział lub zbyt niskie coverage oznacza `NON_EVALUABLE`, nie zero i nie safe value.

### B3 — buy/sell flow

Co najmniej:

```text
buy_count / sell_count
buy_quote_flow / sell_quote_flow
net_quote_flow
buy transaction share
buy quote share
buyer-weighted i cluster-weighted concentration
trade rate
flow acceleration między anchorami
```

Quote flow pochodzi z decoded successful swap amounts. Unsigned reserve movement proxy nie może być nazwany volume, OFI ani buy/sell flow.

### B4 — high-specificity creator/funding risk

Pierwsza faza B4 jest flagą/feature group, nie hard veto.

Kandydaci obejmują między innymi:

```text
direct creator/deployer → early buyer funding
funding in the same transaction / same block
shared immediate funding hub
creator-controlled buyer cluster z wysokim evidence confidence
```

Szersze klasy — generic same-block activity, creator self-buy proxy, bundle suspicion, early concentration — nie są automatycznie hard rejectem.

Hard veto może być zaproponowane dopiero, gdy osobna paired ablation wykaże:

- dodatni `LCB95(ΔEV)` względem użycia tej samej informacji jako soft risk feature;
- wysoką i stabilną specificity;
- akceptowalny false-reject economic harm;
- stabilność per regime;
- brak zależności od jednego klastra/deployera;
- prospektywne potwierdzenie.

### B5 — price/trajectory

Co najmniej:

```text
return from T0
return from first executable decision state
reserve velocity
price/market-cap velocity
acceleration
drawdown from local peak
bonding progress
path shape summaries
```

B5 przechodzi wyłącznie, jeśli wnosi incremental value po B4. Price, buyer count, buy count, volume i bonding progress nie są traktowane jako niezależne kanały tylko dlatego, że mają różne nazwy.

### B6 — cutoff-safe metadata / attention

Osobna warstwa obejmuje creation-time metadata dostępne przed cutoffem:

```text
Telegram presence
Twitter/X presence
website presence
metadata completeness
URI fetch status i observed-at time
creator history dostępna przed launch
initial market-cap / initial reserve state
```

Późniejsze zmiany metadata, stan strony odczytany po cutoffie, liczba followersów z przyszłości i post-launch social activity są zakazane.

Social presence jest sygnałem predykcyjnym do testu, nie dowodem jakości ani przyczynowości.

## 9. Model candidates

Eksperyment zaczyna od modeli prostych. Dopuszczalna kolejność:

```text
M0: progi/ranking jednowymiarowy
M1: regularized linear/logistic/quantile models
M2: negative-binomial / finite count mixture
M3: Cox/latent-intensity/regime model
M4: tree boosting z monotonic/complexity constraints
M5: HawkesN lub inny finite-population self-exciting model
```

Klasyczny stacjonarny Hawkes nie jest modelem domyślnym. Proces nowych buyers może zawierać self-excitation i równocześnie finite-population depletion, heterogeniczność launchy, regime switching, batching i coordinated cohorts.

Model bardziej złożony przechodzi tylko, gdy poprawia jednocześnie:

- out-of-sample likelihood albo właściwą probabilistic loss;
- calibration;
- ranking przy zamrożonym coverage;
- primary executable net EV;
- stabilność czasową i reżimową;
- prospective result.

Lepszy fit timestampów bez poprawy EV nie uzasadnia wdrożenia modelu.

Każdy model emituje:

```text
score
calibrated probability lub expected-return estimate
uncertainty
model applicability / regime
missingness status
feature version
model artifact hash
```

## 10. Frozen exit-policy set

### 10.1. PRIMARY

`PRIMARY` jest exact polityką posiadającą canonical shadow position authority na freeze commit eksperymentu. Jeżeli authority zmieni się przed zamrożeniem runu, powstaje nowa wersja specyfikacji outcome contract; nie wolno podmienić PRIMARY po otwarciu holdoutu.

Na baseline commit HET-PM V2 pozostaje observe-only, dlatego nie może zostać po cichu potraktowany jako authoritative realized outcome.

### 10.2. Sensitivity policies

Obowiązkowo co najmniej:

```text
K1: aktualny PRIMARY authority
K2: HET-PM V2 observe-only candidate, jeśli posiada kompletny evidence contract
K3: prosty fixed TP/SL + absolute max hold
K4: fixed-time exits dla zamrożonego gridu
```

Polityki, progi, version IDs i config hashes są zamrożone przed holdoutem. Wyniki sensitivity służą do sprawdzenia, czy entry edge nie istnieje wyłącznie pod jednym dopasowanym exitem.

## 11. Execution-truth reconstruction

### 11.1. Entry intent i landing

Dla każdego decision unit:

```text
decision_time
intent_created_time
build_delay
queue_delay
submission_delay
landing_slot/time
confirmation status
```

Historyczny emulator używa dystrybucji opóźnień pochodzącej z rzeczywistych Ghost execution/shadow telemetry, zamrożonej jako:

```text
L50
L90
L99
```

Nie wolno przyjąć arbitralnego stałego `200 ms`, jeżeli nie wynika ono z danych.

### 11.2. Notional profiles

Przed holdoutem zamraża się:

```text
N0 = docelowy mikro-notional
N_low = 0.5 × N0
N_high = 2.0 × N0
```

PRIMARY result używa `N0`. Sensitivity sprawdza nonlinear price impact. Fractional Kelly jest poza zakresem.

### 11.3. Exact execution state

Wejście następuje na pierwszym canonical executable state osiągalnym po landing time. Exact quote uwzględnia:

- właściwy program/pool;
- curve complete/migration state;
- current reserves;
- trade size;
- historical fee schedule;
- network base fee;
- priority fee;
- Jito tip;
- interface/referral fee, jeśli rzeczywiście używana;
- slippage limit;
- stale blockhash/quote invalidation;
- retry policy;
- route transition bonding curve → PumpSwap.

Wyjście używa analogicznego exact sell path. Brak executable route, stale state, failed submit, unknown confirmation albo insufficient liquidity są typed outcomes, nie pomijanymi rekordami.

### 11.4. Zakazane uproszczenia

Zakazane jako primary evidence:

```text
entry po mark price
sell po ostatniej obserwowanej cenie
stały round-trip fee bez route/state
uznanie submit za fill
uznanie unknown za success
pominięcie failed entries z denominatora
pominięcie failed exits z PnL
interpolacja przez migration gap
użycie przyszłej rezerwy do quote w wcześniejszym czasie
```

## 12. Data split i leakage control

### 12.1. Nested purged group walk-forward

Wymagany jest nested walk-forward:

```text
outer: chronological train → validation/test windows
inner: model/feature/hyperparameter selection wyłącznie w historycznej części outer train
purge: wszystkie powiązane grupy przecinające granicę
embargo: launch window po granicy splitu
```

Group identities obejmują co najmniej:

```text
creator/deployer
known funding root/family
persistent buyer cohort
high-confidence connected wallet component
```

Launch z grupą obecną po obu stronach splitu jest przypisany do późniejszej strony albo usuwany z wcześniejszej zgodnie z frozen purge rule. Nie wolno pozwolić, aby ten sam cabal uczył model i jednocześnie udowadniał jego generalizację.

### 12.2. Final untouched holdout

Final holdout:

- jest najnowszym chronologicznym blokiem danych;
- ma osobny manifest i hash;
- nie jest używany do wyboru cech, anchorów, coverage, modelu, exitu ani kosztów;
- jest otwierany raz przez frozen evaluation tool;
- każda zmiana po otwarciu wymaga nowego holdoutu i nowej spec version;
- zachowuje grupowe purge/embargo.

### 12.3. Prospective holdout

Po historical PASS uruchamiane są co najmniej dwa niezależne prospective shadow runs z różnymi `run_id` i `launch_cohort_id`. Candidate/model/config są immutable między runami poza z góry dopuszczonym operational fixem, który nie zmienia wyników i wymaga pełnego restartu evidence sequence.

## 13. Statistical evaluation contract

### 13.1. Primary metrics

Raportuje się:

```text
absolute net EV per accepted trade
paired incremental net EV versus B0 i versus B(n-1)
accepted-trade count
market coverage
capital-weighted EV
failure-adjusted executable EV
LCB/CI dla primary metrics
```

### 13.2. Coverage buckets

Obowiązkowo:

```text
0.1%
0.5%
1%
2%
5%
```

Primary promotion coverage jest wybierane i zamrażane na train/validation przed otwarciem holdoutu. Pozostałe coverage są sensitivity, nie okazją do wybrania najlepszego wyniku po fakcie.

Bucket jest `INSUFFICIENT_EVIDENCE`, jeśli posiada mniej niż:

```text
200 accepted decision units
50 independent launch-group clusters
10 distinct calendar days
```

Dla finalnego prospective combined gate wymagane jest co najmniej:

```text
500 accepted decision units łącznie
minimum 150 na każdy niezależny validation run
minimum 100 independent creator/funding/cohort groups łącznie
```

Jeżeli wybrany coverage nie osiąga minimum w uzgodnionym horyzoncie, wynik pozostaje niewystarczający; nie wolno zwiększyć coverage po zobaczeniu PnL bez restartu freeze.

### 13.3. Uncertainty

Zwykły IID row bootstrap jest niedopuszczalny jako główny CI.

Wymagane są:

```text
launch-cluster paired bootstrap
creator/funding/cohort cluster bootstrap
moving-block albo stationary block bootstrap po czasie
```

Gate używa najbardziej konserwatywnego prawidłowego LCB spośród pre-registered metod. Effective sample size i concentration diagnostics muszą zostać pokazane obok nominalnego `n`.

### 13.4. Multiple testing

Przed discovery freeze utrwala się:

- liczbę anchorów;
- feature ladder;
- model families;
- coverage buckets;
- latency/cost/notional profiles;
- exit policies;
- primary hypothesis family;
- alpha-spending lub korektę wielokrotnych testów.

Wynik wybrany spośród wielu konfiguracji bez korekty nie jest promotion evidence.

### 13.5. Tail i concentration metrics

Obowiązkowo:

```text
CVaR lower-tail 20%
worst-decile return
p01/p05/p10
maximum drawdown portfela przy chronologicznym replay
trimmed mean EV
top-1 i top-3 positive-PnL contribution
top creator/funder/cohort accepted-trade concentration
weekly/regime breakdown
```

Candidate nie przechodzi, jeśli dodatni wynik jest napędzany pojedynczym ekstremum, jedną grupą albo wąskim reżimem niewykrywalnym decision-time.

Normatywne concentration ceilings dla primary gate:

```text
top-1 connected group ≤ 10% accepted trades
top-3 connected groups ≤ 25% accepted trades
top-3 profitable trades ≤ 35% całkowitego positive PnL
trimmed mean net EV > 0
```

### 13.6. Stress tests i negative controls

Obowiązkowo:

- label permutation;
- time-shift features;
- shuffled buyer identities w obrębie dopuszczalnych bloków;
- anchor perturbation;
- latency `L50/L90/L99`;
- current fees oraz adverse fee/cost scenario;
- notional `N_low/N0/N_high`;
- usunięcie top profitable observations;
- leave-one-week-out;
- leave-one-large-cohort-out;
- missingness perturbation;
- activity-matched placebo dla cohort claims.

Jeżeli shuffled/permuted pipeline daje podobne EV albo ranking, sygnał jest nieważny.

## 14. Predictive claim kontra causal claim

Trading claim wymaga:

```text
cutoff-safe availability
out-of-time incremental executable EV
stability
operational feasibility
prospective confirmation
```

Nie wymaga udowodnienia, że buyer cohort przyczynowo wywołuje pompę.

Causal claim wymaga osobnego projektu: propensity/matching, negative controls, alternative explanations i odpowiednich assumptions. Activity-matched placebo może obalić prostą narrację przyczynową, ale nie automatycznie wartość predykcyjną.

W raportach należy jawnie oznaczać:

```text
PREDICTIVE_ASSOCIATION_ONLY
CAUSAL_CLAIM_NOT_MADE
```

chyba że powstanie osobny, zaakceptowany causal protocol.

## 15. Missingness, quality i fail-closed

Każda cecha posiada:

```text
value albo null
availability
measurement_quality
source cutoff
producer/config/version provenance
typed reason codes
```

Zasady:

- `None` nie jest zerem;
- unknown funding nie jest independent;
- unknown creator nie jest clean creator;
- non-clean group status nie tworzy clean field evidence;
- historyczne legacy zero/false nie dowodzi pomiaru;
- mixed config/schema identity dyskwalifikuje run;
- brak exact source cutoff dyskwalifikuje feature;
- niedostępna cecha nie może być imputowana outcome-aware metodą;
- model musi działać w jawnie zdefiniowanym missingness regime albo zwrócić non-evaluable.

## 16. Artefakty wymagane od planu wykonawczego

Plan wykonawczy musi przypisać właścicieli i exact schemas co najmniej dla następujących rodzin.

Dataset pod:

```text
datasets/selector/<scope>/
```

Wymagane logical artifacts:

```text
candidate_universe_v2.jsonl
event_ledger_v1.jsonl
feature_anchor_snapshots_v2.jsonl
buyer_cluster_context_v2.jsonl
metadata_attention_context_v1.jsonl
executable_entry_outcomes_v1.jsonl
executable_path_outcomes_v1.jsonl
ablation_scored_units_v1.jsonl
prospective_shadow_units_v1.jsonl
```

Reports pod:

```text
reports/selector/<scope>/
```

Wymagane:

```text
experiment_freeze_manifest_v1.json
universe_coverage_audit_v1.json
decoder_protocol_registry_v1.json
fee_schedule_registry_v1.json
cutoff_leakage_audit_v2.json
execution_reconstruction_audit_v1.json
cluster_quality_audit_v1.json
split_group_purge_manifest_v1.json
ablation_incremental_ev_report_v1.json
model_calibration_report_v1.json
stress_negative_control_report_v1.json
untouched_holdout_gate_v1.json
prospective_multi_run_gate_v1.json
```

Nazwy mogą zostać skorygowane w planie wyłącznie przy zachowaniu jednoznacznego mapowania i wszystkich semantyk.

Każdy manifest zachowuje:

```text
input path / size / SHA-256
run_id
launch_cohort_id
base commit
analysis tool commit/hash
schema versions
program/decoder registry hash
fee registry hash
feature registry hash
model artifact hash
exit policy/config hashes
latency/notional profile hashes
split/purge/embargo contract hash
criteria hash
```

Identyczne inputy, criteria i tool versions muszą tworzyć deterministycznie równoważny raport semantyczny.

## 17. Fazy eksperymentu

### T0 — specification freeze

Ten dokument. Brak zmian runtime.

### T1 — universe, decoder i protocol truth

Cel:

- pełny SOL launch universe;
- exact identity/order;
- protocol/IDL/fee registry;
- coverage audit;
- decoder golden fixtures;
- brak feature/model work przed PASS T1.

### T2 — cutoff-safe evidence i execution truth

Cel:

- wszystkie anchors;
- B0-B6 raw evidence;
- cluster bounds;
- metadata observed-at proof;
- executable entry/exit reconstruction;
- leakage i execution audit;
- frozen outcome vector.

### T3 — historical discovery

Cel:

- paired B0-B6 ablations;
- simple-to-complex model comparison;
- nested purged group walk-forward;
- negative controls;
- wybór jednego candidate package;
- bez otwierania final holdoutu.

### T4 — candidate freeze

Zamrożone zostają:

```text
feature set
anchor/stopping rule
model artifact
threshold/coverage
PRIMARY exit i sensitivity exits
latency/cost/notional profiles
missingness policy
criteria
analysis tool
```

### T5 — untouched historical holdout

Jednorazowa ewaluacja frozen package. FAIL kończy ścieżkę promocji. Poprawki wracają do nowej wersji T3/T4 i wymagają nowego holdoutu.

### T6 — prospective shadow validation

Co najmniej dwa niezależne runy, pełny universe capture, processing-time anchors, real telemetry latencji, zero authority change.

### T7 — separate promotion decision

Dopiero po PASS T6 może powstać osobny policy-promotion plan. T7 nie jest autoryzowane przez ten dokument.

## 18. Bramki PASS/FAIL

### Gate 0 — architecture

PASS, gdy:

- brak drugiego scoring stacku;
- MFS/selector SSOT zachowane;
- brak authority change;
- Type5/Gatekeeper V3 pozostają integration target;
- plan jest additive, shadow-first i replay-safe.

### Gate 1 — universe i ingest truth

PASS, gdy:

- universe nie pochodzi z accepted/outcome rows;
- wszystkie in-scope periods mają coverage proof;
- gaps, reconnects i noncanonical backfills są jawne;
- identity collisions = 0 w clean denominator;
- decoder variants i program regimes są kompletne;
- source ordering jest deterministyczny;
- historical timestamp limitations są jawne.

### Gate 2 — cutoff i execution truth

PASS, gdy:

- leakage audit = PASS;
- każdy feature ma source cutoff;
- każdy outcome zaczyna się po decision cutoff;
- exact fee/program/route registry join = 100% dla evaluable units;
- failed/unknown execution pozostaje w denominatorze;
- mark-only nie jest raportowany jako net PnL;
- frozen exit replay jest deterministyczny.

### Gate 3 — discovery/falsification

PASS, gdy co najmniej jedna kolejna warstwa ma:

```text
LCB95 paired ΔEV > 0 po multiple-testing control
absolute net EV > 0 na outer folds
negative controls fail to reproduce signal
calibration acceptable dla deklarowanego outputu
no material tail deterioration
no concentration breach
operationally computable before deadline
```

Discovery PASS nie jest promotion PASS.

### Gate 4 — untouched historical holdout

Frozen candidate przechodzi wyłącznie, gdy:

```text
LCB95 absolute net EV > 0
LCB95 paired ΔEV vs B0 > 0
LCB95 paired ΔEV vs poprzednia warstwa > 0
primary result dodatni w latency L90
current-cost i adverse-cost scenario nie odwracają root claim
CVaR20 nie jest statystycznie gorszy od B0
trimmed mean EV > 0
co najmniej 70% evaluable calendar weeks ma ΔEV ≥ 0
concentration ceilings zachowane
sample floors zachowane
all audits PASS
```

### Gate 5 — prospective shadow

PASS wymaga:

- minimum dwóch independent runów i launch cohorts;
- immutable candidate/config/tool identity;
- pełnego denominator capture;
- zero runtime paniców i clean shutdown;
- exact writer/join/replay coverage;
- każdy run ma dodatnie mean net EV dla frozen package;
- combined conservative `LCB95 absolute net EV > 0`;
- combined `LCB95 paired ΔEV vs B0 > 0`;
- L90 operational result dodatni;
- tail/concentration/stability gates PASS;
- historical-to-prospective drift nie przekracza pre-registered bounds;
- brak evidence, że model wykorzystuje niedostępny live input.

### Gate 6 — operational feasibility

PASS, gdy:

- feature computation mieści się w frozen observation deadline;
- brak hot-path RPC wymagającego nieograniczonego wait;
- memory/CPU/queue lag są bounded;
- missing data prowadzi do typed non-evaluable/degraded, nie panicu;
- replay i online wynik są semantycznie równoważne;
- rollout może być observe-only bez dual authority.

### Gate 7 — promotion

Nie jest częścią tego eksperymentu. Wymaga osobnego planu, kryteriów rollback, staged rollout i decyzji authority.

## 19. Jawne kryteria odrzucenia całej hipotezy

Hipotezę należy zamknąć jako `REJECTED` albo `NO EVIDENCE`, jeżeli wystąpi którekolwiek:

- żaden B1-B6 nie daje dodatniego conservative LCB incremental EV;
- wynik znika po exact costs/latency;
- wynik znika po group purge;
- wynik jest podobny do permutation/placebo;
- wynik jest napędzany top ekstremami lub jednym cabalem;
- signal works only in regime niewykrywalnym decision-time;
- wymagany input jest zbyt wolny albo niedostępny live;
- funding coverage czyni EIC non-evaluable dla większości candidate units;
- historical PASS nie replikuje się prospectively;
- mark opportunity jest dodatnia, lecz executable EV pozostaje ujemne;
- poprawa graduacji nie przekłada się na net PnL;
- hard veto poprawia precision, ale niszczy economic EV przez false rejects.

Prawidłowe odrzucenie słabego sygnału jest pełnoprawnym wynikiem eksperymentu.

## 20. Wymagania wobec planu wykonawczego

Plan napisany na podstawie tego dokumentu musi zawierać:

1. dokładny baseline audit na bieżącym `main`;
2. mapę reuse istniejących producerów i jawne missing primitives;
3. brak duplicate metric producers;
4. commit-by-commit/PR-by-PR decomposition;
5. exact Rust/Python/JSON schema ownership;
6. canonical event and account source mapping;
7. historical data acquisition i storage budget;
8. protocol/IDL/fee upgrade handling;
9. exact execution emulator design i parity tests z runtime quote builders;
10. feature registry B0-B6 i cutoff contracts;
11. clustering algorithm, bounds i quality contracts;
12. metadata fetch observed-at contract;
13. split/purge/embargo algorithm;
14. pre-registration i multiple-testing policy;
15. sample-size/power simulation potwierdzającą lub podnoszącą floors z sekcji 13.2;
16. deterministic manifests i report validators;
17. unit, property, golden, replay, leakage i negative-control tests;
18. resource budgets i hot-path isolation;
19. prospective run profiles, shutdown i writer-health;
20. failure classification i fail-closed semantics;
21. rollback do baseline bez migracji authority;
22. osobny zakres późniejszego promotion planu.

Plan nie może rozpocząć strojenia modelu ani progów przed PASS universe, cutoff i execution-truth gates.

## 21. Facts context — niewiążące dla targetu EV

Na dzień dokumentu oficjalne źródła i najnowsze preprinty wskazują:

- bonding-curve fee `1.25%` per trade;
- canonical PumpSwap dynamic fees `1.25% → 0.30%`;
- graduation rate `0.198%` w próbce 832 941 launchy z maja–czerwca 2026;
- historyczną medianę czasu do graduacji około `4.4 min` w innej próbce;
- silną asocjację social presence i initial mcap z graduacją;
- istnienie persistent buyer cohorts oraz problem naiwnej interpretacji przyczynowej.

Te fakty są kontekstem i źródłem auxiliary hypotheses. Graduation base rate `0.198%` nie jest base rate'em głównego targetu. Właściwy profitable-trade base rate musi zostać zmierzony dla exact `PRIMARY`, notional, latency, costs i horizon.

Preprint nie jest niezależną replikacją ani promotion evidence Ghosta.

## 22. Źródła

Źródła protokołu:

- Pump fees: `https://pump.fun/docs/fees`
- Pump bonding curve: `https://pump.fun/docs/bonding-curve`
- Pump public docs / program and instruction interfaces: `https://github.com/pump-fun/pump-public-docs`

Źródła badawcze — wyłącznie kontekst / auxiliary hypotheses:

- Kamat, *Pump.fun Graduation Regime Windows*, arXiv `2607.02823`.
- Marino et al., *Predicting the success of new crypto-tokens: the Pump.fun case*, arXiv `2602.14860`.
- Kamat, *Coordinated Sniper Cohorts on Pump.fun*, arXiv `2607.02795`.

Źródła repozytorium, które plan ma zreconciliationować:

- `AGENTS.md`;
- `PLANS/DO_REALIZACJI/PLAN_TYPE5_V3_INTEGRATION_AFTER_METRIC_CONTRACTS_V1_1_20260711.md`;
- `PLANS/PLAN_SELECTOR_DATASET_V2_PHASE0_TO_PHASE4_20260601.md`;
- `PLANS/DO_REALIZACJI/POSITION_MANAGER_HET_V2.md`;
- `docs/ADR/ADR_8D_HET_PM_V2_PR_B_PROMOTION_EVIDENCE_PREREQUISITE_20260717.md`;
- `ghost-core/src/checkpoint/types.rs`;
- `ghost-launcher/src/session/observation.rs`;
- `ghost-launcher/src/components/gatekeeper_v3.rs`;
- `scripts/selector_pipeline_common.py`;
- `scripts/build_selector_training_view.py`;
- `scripts/build_selector_r2_market_paths.py`;
- `scripts/build_selector_buyer_quality_context.py`;
- `scripts/build_selector_funding_graph_context.py`.

## 23. Finalny werdykt T0

```text
DOCUMENT_READY_AS_IMPLEMENTATION_PLAN_INPUT
HYPOTHESIS_NOT_VALIDATED
NO_THRESHOLDS_PROMOTED
NO_MONTE_CARLO_EV_ACCEPTED
NO_HAWKES_ASSUMPTION
NO_RAW_WALLET_INDEPENDENCE_CLAIM
NO_GRADUATION_TARGET_PROMOTION
NO_KELLY_SIZING
NO_POLICY_AUTHORITY_CHANGE
```

Następny legalny krok:

> napisać osobny plan wykonawczy, który mapuje tę specyfikację na minimalną, etapową implementację evidence-first; najpierw universe i execution truth, potem discovery, na końcu prospective shadow. Żaden etap planu nie może przeskoczyć bramek T1/T2 ani rozpocząć live/policy promotion.