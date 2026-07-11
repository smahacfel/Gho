# PR0: baseline reconciliation kontraktów interpretacyjnych metryk

Status:

```text
BASELINE_RECONCILIATION_PASS
PR1_FOUNDATION_ALLOWED
RUNTIME_AND_POLICY_UNCHANGED
```

Data audytu: 2026-07-11

Repozytorium: `/root/Gho_dynamic_exit_v1`

Branch: `codex/executable-dynamic-exit-sidecar-v1`

Audytowany commit i merge-base z upstream:
`f3318f3a71a9202ced7af9cf43c064fa9f2f0c4a`

Plan normatywny:
`PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`

Raport źródłowy:
`PLANS/AUDYT/RAPORT_AUDYT_KOREKTY_INTERPRETACJI_METRYK_20260710.md`

Zakres: dokładnie dziesięć rodzin metryk z planu V1.1 oraz przekrojowe
kontrakty statusów, schema, loggera i replayu potrzebne do ich bezpiecznej
implementacji. PR0 nie zmienia Rust, TOML, progów, wag, verdictów, reason codes,
selector score ani shadow/live behavior.

## 1. Werdykt wykonawczy

Baseline został pogodzony z aktualnym kodem. Można rozpocząć PR1, ponieważ:

- dla każdej z dziesięciu rodzin wskazano rzeczywistego producenta, granicę MFS,
  aktywnych i kompatybilnościowych konsumentów oraz brakujące elementy;
- rozdzielono elementy już gotowe od częściowych, błędnych semantycznie i
  brakujących;
- potwierdzono kolizję powierzchni dev-buy oraz aktywny odczyt Phase 5;
- potwierdzono, że top3 helper już istnieje i nie wolno go implementować drugi
  raz;
- potwierdzono FTDI value/actionability mismatch;
- zinwentaryzowano równoległe status enums oraz schema/replay versions;
- historyczna wykonalność została oceniona osobno, bez użycia danych
  feasibility jako przyszłych danych walidacyjnych.

`BASELINE_RECONCILIATION_PASS` oznacza kompletność i rzetelność PR0. Nie
oznacza gotowości do burn-in, policy promotion ani cutover. Te pozostają
zablokowane przez PR1, PR2A, PR2B, PR2C i prospective burn-in.

## 2. Metoda i granice dowodu

Wnioski oparto na trzech rozdzielonych warstwach:

1. Kod/formuła: definicje typów, producenci, materializacja i rzeczywiste
   callsite'y aktywnej policy.
2. Testy: istniejące kontrakty jednostkowe i integracyjne uruchomione na
   audytowanym checkoutcie.
3. Logi: schema i field presence w stabilnych historycznych v33 JSONL.

Kod dowodzi definicji i ścieżki wykonania. Test dowodzi zachowania objętego
fixture'em. Obecność pola w logu nie dowodzi poprawności semantyki producenta.
Historyczny brak delty nie jest formalnym dowodem równoważności.

Worktree był przed PR0 nieczysty. Wszystkie zastane zmiany użytkownika pozostają
poza zakresem; commit i merge-base są równe, więc baseline źródłowy odnosi się do
tego samego commita, a stan untracked został potraktowany tylko jako jawnie
wskazane dokumenty i artefakty wejściowe.

## 3. Macierz reconciliation

Klasyfikacja główna opisuje gotowość całego kontraktu V1.1, nie tylko istnienie
legacy scalara.

| # | Kontrakt | Klasyfikacja główna | Istniejący status powierzchni | Najważniejszy implementation delta |
| ---: | --- | --- | --- | --- |
| 1 | FTDI runtime value/actionability | `PARTIALLY_SATISFIED` | value istnieje; actionability `INCORRECT` | typed value; osobne legacy i unique-buyer actionability; profile authority |
| 2 | dev first-observed/primary | `PARTIALLY_SATISFIED` | obie definicje istnieją; publiczne nazwy kolidują | surface-qualified registry, MFS primary evidence i explicit effective policy source |
| 3 | same-ms exact/cluster/recent | `PARTIALLY_SATISFIED` | trzy obliczenia istnieją pod przeciążonymi nazwami | typed source/population/window/denominator/provenance |
| 4 | top3 preferred ratio/legacy alias | `PARTIALLY_SATISFIED` | helper i aktywne odczyty `ALREADY_SATISFIED` | callsite/static audit, mismatch telemetry i parity proof; bez nowego helpera |
| 5 | legacy flip/flip V2 | `PARTIALLY_SATISFIED` | legacy hybrid istnieje; V2 `MISSING` | normatywny V2 state machine, dedupe/order/success/dust/status evidence |
| 6 | legacy FSC/FSC v2 | `PARTIALLY_SATISFIED` | oba obliczenia istnieją | typed legacy envelope i jawna authority separation |
| 7 | `evidence_status.fsc` | `INCORRECT` | legacy compatibility status istnieje | `fsc_legacy` i `fsc_v2`; frozen v1 adapter; v2 default non-required |
| 8 | manipulation flags/numerics | `PARTIALLY_SATISFIED` | raw wartości są używane; `high_*` evidence `INCORRECT` | presence-aware numerics i derived flags z provenance |
| 9 | reserve velocity | `PARTIALLY_SATISFIED` | scalar/formuła istnieją; status `MISSING` | interval, sample count, validity, source i fallback reason |
| 10 | recent buy/sell | `PARTIALLY_SATISFIED` | legacy scalar `LOGGING_ONLY`; nazwa `INCORRECT` | counts, optional unbounded ratio i bounded buy share |

Przekrojowo `MetricEvidenceEnvelopeV1`, authority profile i typed adapter matrix
są `MISSING`; istniejące status enums są poprawnymi lokalnymi typami, ale nie
tworzą jednego kanonicznego kontraktu availability/quality/actionability.

## 4. Canonical runtime i authority baseline

Aktywna ścieżka terminalna została potwierdzona jako:

```text
OracleRuntime::evaluate_feature_driven_terminal_verdict
→ GatekeeperBuffer::evaluate_from_features
→ build_assessment_from_features
→ MaterializedFeatureSet
```

Dowody: `ghost-launcher/src/oracle_runtime.rs:16901`,
`ghost-launcher/src/oracle_runtime.rs:16976`,
`ghost-launcher/src/components/gatekeeper.rs:5359` i
`ghost-launcher/src/components/gatekeeper_policy.rs:1421`.

`MaterializedFeatureSet` pozostaje canonical decision snapshot
(`ghost-core/src/checkpoint/types.rs:888`). GatekeeperBuffer ma dozwolone
compatibility/accumulation surfaces, ale nie może stać się drugim SSOT ani
przejąć authority od snapshotu podawanego do `evaluate_from_features`.

W repo nie istnieją jeszcze `metric_contract_rollout_mode`, hashowany
`metric_contract_profile` ani per-entry `MetricAuthorityClass`. Globalne
Legacy/DualCompute/V2 i Profile A należy więc dodać w PR1 jako nowe,
serde-compatible foundation, bez zmiany aktualnych odczytów policy.

## 5. Reconciliation per kontrakt

### 5.1 FTDI: runtime value kontra actionability

Requirement V1.1:

- runtime value = `unique_topologies / unique_successful_buyers` po pierwszej
  kwalifikującej próbce per buyer;
- coordination HHI pozostaje oddzielnym export-only kontraktem;
- legacy actionability i corrected unique-buyer actionability muszą być
  rozdzielone, a corrected gate pozostaje counterfactual.

Current implementation:

- `unique_buyer_samples()` wybiera pierwszą próbkę per buyer;
- `compute_ftdi_from_buys()` liczy topologie na `unique_samples` i dzieli przez
  `unique_samples.len()` (`sybil_metrics.rs:351-387`);
- clean/degraded jest jednak wyznaczane przez
  `stats.buy_sample_count < 3`, czyli całkowitą liczbę BUY, a nie liczbę
  unikalnych buyerów (`sybil_metrics.rs:22,362`);
- MFS otrzymuje FTDI przez materializację sesji; aktywna policy sprawdza field
  presence oraz próg FTDI (`gatekeeper_policy.rs:3010,3077-3078`);
- coordination-risk ma osobne HHI/diversity helpers i pozostaje export-only.

Konsekwencja: sekwencja trzech BUY od dwóch buyerów może mieć wartość z
mianownikiem 2, ale legacy quality gate uznać próbkę za clean dzięki count=3.

Klasyfikacja:

- runtime value: `ALREADY_SATISFIED`;
- legacy actionability: `INCORRECT` względem docelowej interpretacji;
- typed evidence/profile split: `MISSING`;
- cały kontrakt: `PARTIALLY_SATISFIED`.

Delta PR2A: zachować legacy behavior jako authoritative, dodać jawne
`legacy_buy_tx_count_actionability` i counterfactual
`unique_buyer_actionability`, z population/denominator/sample count i typed
reason. Nie używać coordination HHI jako runtime FTDI.

### 5.2 Dev-buy: kolizja fizycznych powierzchni

Requirement V1.1: rozdzielić first observed creator buy, create-signature
anchored primary creator buy, MFS evidence i effective policy read.

Current implementation:

- TxIntelligence zapisuje pierwszą zaobserwowaną wielkość BUY w
  `SignerStats.first_buy_volume_sol` (`engine.rs:204-214`) i mapuje ją do
  `dev_buy_total_sol` (`engine.rs:653-669`), następnie do
  `TxIntelFeatures.dev_buy_sol` (`engine.rs:312`, typ w
  `ghost-core/src/tx_intelligence/types.rs:71`);
- ten path filtruje duplicate key i dust przed SignerStats, ale nie odrzuca
  nieudanej transakcji przed akumulacją: `!tx.success` zwiększa failed count,
  po czym kod nadal aktualizuje BUY/signer stats (`engine.rs:150-219`);
- GatekeeperBuffer ma osobny `find_primary_creator_buy_index()`, preferujący
  create signature, a przy jej braku najwcześniejszy creator BUY
  (`gatekeeper.rs:4937-4986`), i zapisuje tę wartość do pola o nazwie
  `dev_buy_total_sol` (`gatekeeper.rs:4899-4916`);
- aktywne Phase 5 nie czyta tego primary pola. `dev_behavior_from_features()`
  mapuje `features.tx_intel_features.dev_buy_sol` do lokalnego
  `dev_buy_total_sol` (`gatekeeper_policy.rs:2911-2914`), używanego przez
  `min_dev_buy_sol/max_dev_buy_sol` (`gatekeeper_policy.rs:1459,1466-1467`).

Aktualny authority:

| Surface | Semantyka | Authority |
| --- | --- | --- |
| TxIntel `first_buy_volume_sol` | first observed BUY per resolved dev signer | producer aktywnego MFS field |
| TxIntel/MFS `dev_buy_sol` | first observed dev BUY, legacy zero fallback | aktywny Phase 5 authority |
| GatekeeperBuffer `dev_buy_total_sol` | create-signature primary, fallback earliest creator BUY | compatibility/accumulator, nie aktywny MFS authority |
| MFS primary field V1.1 | nie istnieje | `MISSING`, przyszły counterfactual |
| effective policy source/profile | implicit TxIntel field | jawny selector `MISSING` |

Klasyfikacja: `PARTIALLY_SATISFIED`, z `INCORRECT` naming collision. Oba
algorytmy istnieją, lecz nie są surface-qualified, a primary nie jest
materializowany jako odrębne evidence.

Delta PR1/PR2A: registry musi użyć nazw z planu:
`tx_intel_dev_first_observed_buy_sol`,
`gatekeeper_buffer_dev_primary_buy_sol`,
`mfs_dev_first_observed_buy_sol`, `mfs_dev_primary_buy_sol_v1` i
`effective_policy_dev_buy_sol`. Comparator baseline porównuje candidate z
rzeczywistym aktywnym MFS read. Primary pozostaje counterfactual; nie zmieniać
Phase 5 ani progów. PR2A musi również jawnie zdefiniować eligibility/success dla
first-observed V2, zamiast dziedziczyć nieudokumentowane legacy behavior.

### 5.3 Same-ms: trzy różne populacje

Current definitions:

| Surface | Definicja | Populacja/mianownik | Authority |
| --- | --- | --- | --- |
| `TxIntelFeatures.same_ms_tx_ratio` | liczba adjacent exact `delta_ms == 0` / `tx_count` | TxIntel deduped non-dust stream, w legacy także failed tx; denominator transaction count | aktywny MFS/policy input |
| `SignerDiversity.same_ms_tx_ratio` | adjacent `delta_ms < 50` / transaction count | diversity input | helper o semantyce cluster, nie exact |
| `same_ms_tx_ratio_recent` | extra events per identical timestamp / successful tx count | recent wall-clock RCE window | `LOGGING_ONLY` |

Dowody: TxIntel materializacja `engine.rs:308`, timing recompute
`engine.rs:675-706`; cluster threshold `analysis.rs:3,219-238`; recent exact
`session/observation.rs:1434-1503`.

Wszystkie trzy obliczenia istnieją, ale nazwa sama nie identyfikuje source,
population, time contract ani denominatora. Legacy exact denominator zostaje
zachowany dla parity i powinien być nazwany uczciwie jako
`adjacent_collision_count / transaction_count`, a nie liczba wszystkich par.

Klasyfikacja: `PARTIALLY_SATISFIED`.

Delta PR2A: osobne typed contracts `same_ms_exact_legacy`,
`bundle_cluster_lt_50ms` i `recent_same_timestamp_extra_ratio`, każdy z source,
window, count, denominator i quality. Tylko exact legacy może wejść do przyszłego
equivalence lane.

### 5.4 Top3: preferred ratio i compatibility alias

Current implementation jest dalej niż zakładał pierwotny plan:

- `TxIntelFeatures.top3_signer_volume_ratio: Option<f64>` i legacy
  `top3_volume_pct: f64` istnieją (`types.rs:65-70`);
- `effective_top3_signer_volume_ratio()` preferuje nowe pole i fallbackuje do
  legacy aliasu (`types.rs:103-114`);
- producent liczy udział top3 signer volume w total volume jako ratio 0..1
  (`analysis.rs:212-238`) i zapisuje oba pola (`engine.rs:310-311`);
- TxIntelligence risk flags i aktywne Gatekeeper callsite'y używają helpera,
  m.in. `engine.rs:538`, `gatekeeper_policy.rs:1068,2074,2246,2886,2992`;
- decision schema v33 już serializuje preferred field i zachowuje alias.

Klasyfikacja:

- preferred field/helper/fallback i podstawowe callsite'y:
  `ALREADY_SATISFIED`;
- pełny static callsite guard, mismatch telemetry i selector parity artifact:
  `MISSING`;
- cały kontrakt PR1/PR2A: `PARTIALLY_SATISFIED`.

Delta: nie implementować helpera ponownie. PR1 ma zamknąć exhaustive read audit i
guard przed bezpośrednim policy read legacy aliasu. PR2A dodaje mismatch
telemetry. Nazwa `pct` pozostaje compatibility aliasem ratio-scale.

### 5.5 Flip: legacy hybrid i brak V2

Legacy `flip_ratio_10s` nie jest literalnym, samowystarczalnym wall-clock
kontraktem:

- agregator przyjmuje eventy do globalnego pool window `t0 + window_secs`
  (`early_fingerprint.rs:374-381`);
- per owner kumuluje wszystkie dodatnie i ujemne token deltas, zapisuje
  `first_buy_slot` oraz `last_sell_slot` (`early_fingerprint.rs:407-424`);
- owner jest flipperem, gdy cumulative sold przekracza procent cumulative
  bought i `last_sell_slot - first_buy_slot <= max_flip_slots`
  (`early_fingerprint.rs:773-792`);
- runtime używa domyślnie 10 s, 50% i 20 slotów, lecz nazwa pola nie koduje tych
  ustawień;
- `TxIntelligenceEngine::on_transaction()` przekazuje event do fingerprintu
  przed tx-key dedupe, dust filter i success handling (`engine.rs:150-176`),
  więc legacy fingerprint nie dziedziczy późniejszych zabezpieczeń TxIntel.

Legacy tests potwierdzają slot-gap behavior, lecz nie stanowią specyfikacji V2.

Klasyfikacja:

- legacy fingerprint: `LEGACY_ONLY` i poprawnie zamrożony jako hybrid;
- evidence-only flip V2 state machine: `MISSING`;
- cały kontrakt: `PARTIALLY_SATISFIED`.

Delta PR2B: implementować dokładny automat z planu: canonical ordered eligible
events, stable identity, bounded dedupe, first eligible buy anchor bez re-anchor,
cumulative eligible buys/sells, freeze pierwszego qualifying sell, jednoczesny
wall-clock i slot constraint, fail-closed gap/reconnect/identity statuses. Nie
redefiniować legacy pola i nie promować V2 do policy.

### 5.6 Legacy FSC i FSC v2

Legacy scalar liczy:

```text
1 - distinct_known_sources / known_sources
```

i zwraca `None` dla mniej niż dwóch known sources
(`funding_source.rs:1153-1167`). To count-based collision/compression ratio, nie
HHI ani volume concentration.

FSC v2 już istnieje jako bogate typed evidence z `FscEvidenceStatus`, coverage,
HHI i diagnostics (`ghost-core/src/tx_intelligence/types.rs:205-325`) oraz jest
materializowane obok legacy scalara (`session/observation.rs:2664-2671`). Aktywna
Gatekeeper V2 policy nadal sprawdza legacy scalar
(`gatekeeper_policy.rs:3015,3102-3103`). FSC v2 pozostaje shadow/evidence.

Klasyfikacja: `PARTIALLY_SATISFIED`.

Delta PR2A: zachować legacy formula i authority, dodać typed legacy envelope z
counts/population, a FSC v2 oznaczyć `EvidenceOnly`. Nie implementować ponownie
istniejących v2 typów; adaptować je do canonical status envelope.

### 5.7 `evidence_status.fsc`

V3 materializer ustawia legacy `fsc` jako clean, jeśli sam
`funding_source_concentration` jest `Some`, bez sprawdzenia
`funding_source_v2.status` i coverage (`session/observation.rs:854-873`). Jest to
zgodne z legacy availability aliasem, ale błędne, jeśli konsument interpretuje
status jako jakość FSC v2.

Domyślny Rust `GatekeeperV3EvidenceRequirements` ma `fsc=true`
(`gatekeeper_v3_config.rs:325-380`), podczas gdy aktywny bazowy TOML jawnie
ustawia `fsc=false` (`ghost_brain_config.toml:449-465`). To dodatkowo wymaga
zachowania config provenance: type default nie jest dowodem aktywnego wymogu.

Klasyfikacja: `INCORRECT` jako wspólna nazwa jakości dwóch kontraktów;
legacy compatibility behavior jest historycznie określony, lecz nie może być
używany jako FSC v2 status.

Delta PR2A: dodać `fsc_legacy` i `fsc_v2`, zachować frozen v1 `fsc` adapter,
domyślnie nie wymagać v2 oraz serializować coverage/status oddzielnie.

### 5.8 Manipulation flags i raw numeric presence

`ManipulationContradictionFeatures` przechowuje raw numeric fields jako
`f64` z serde/default zero oraz `high_*` jako bool defaults false
(`ghost-core/src/checkpoint/types.rs:754-809`). Materializer ustawia raw values i
część composite flags, a następnie używa `..Default`, pozostawiając wszystkie
`high_*` false (`session/observation.rs:590-685`).

V3 hard-risk consumer OR-uje `high_*` z bezpośrednimi porównaniami raw values do
config thresholds (`gatekeeper_v3.rs:766-792`). Dzięki temu default-false nie
wyłącza wszystkich obecnych hard checks, ale serialized flag nadal fałszywie
wygląda jak zmierzony false. Dla starego payloadu raw `0.0` nie rozróżnia
missing field od real measured zero.

Klasyfikacja:

- `high_*` jako evidence: `INCORRECT`;
- raw numeric fallback w obecnym V3 consumerze: `PARTIALLY_SATISFIED`;
- presence contract: `MISSING`.

Delta PR2B: `ManipulationNumericEvidenceV2` z `Option<f64>` i per-field status
albo równoważny measured-fields bitset; derived high flags muszą mieć threshold
source/config hash/provenance. V3 v1 replay pozostaje frozen, V2 evidence nie
zmienia authority bez osobnego truth-table equivalence proof.

### 5.9 Reserve velocity

Reducer oblicza zmianę real SOL reserves między dwoma kolejnymi account updates,
dzieloną przez receive-time delta seconds (`reducer.rs:98-120,462-477`). Pierwszy
update oraz `delta_ms == 0` dają `0.0`. MFS fallback bez canonical account state
również daje `0.0` i bootstrap (`session/observation.rs:2820-2893`). Sam scalar
nie pozwala rozróżnić:

- real measured zero;
- first-sample bootstrap;
- zero-duration interval;
- fallback/no canonical update.

To per-update receive-time rate, nie continuous sampler. Nie znaleziono aktywnej
Gatekeeper V2 policy promotion tego pola; jest materialized/V3 evidence.

Klasyfikacja: `PARTIALLY_SATISFIED` — formuła i jednostka istnieją, typed
validity/status `MISSING`.

Delta PR2B: value option, delta reserves, interval_ms, update/sample count,
clock/source, validity i typed fallback reason. Evidence-only.

### 5.10 Recent buy/sell

RCE liczy successful-only BUY i SELL w recent wall-clock window. Gdy sell>0,
`buy_sell_ratio = buy_count / sell_count`; gdy sell=0, zwraca `buy_count` jako
`f64` (`session/observation.rs:1434-1503`). Pole jest emitowane jako
`buy_sell_ratio_recent`, z reason
`rce_a0_not_evaluated_logging_only` (`session/observation.rs:1520-1555`).

Wartość nie jest bounded ratio i przy sell=0 ma inną algebraiczną interpretację.
Nie może być używana jak udział w [0,1].

Klasyfikacja: `PARTIALLY_SATISFIED`; legacy scalar jest `LOGGING_ONLY`, a jego
nazwa/interpretacja jako bounded ratio jest `INCORRECT`.

Delta PR2B: materializować `buy_count`, `sell_count`,
`buy_to_sell_ratio: Option<f64>` (None przy sell=0) i bounded
`buy_share = buy/(buy+sell)`, wraz z window/population/status. Legacy scalar
pozostaje logging-only.

## 6. Canonical status reconciliation

Repo ma równolegle:

- `EvidenceStatus` (`ghost-core/src/checkpoint/types.rs:120`);
- `MetricEvidenceQuality` (`checkpoint/types.rs:186`);
- `FeatureEvidenceStatus` i `MaterializedEvidenceStatus`
  (`checkpoint/types.rs:618,637`);
- `FscEvidenceStatus` (`ghost-core/src/tx_intelligence/types.rs:205`);
- stringowe `degraded_reasons` w sybil/FSC producers.

Nie należy usuwać ich destrukcyjnie. PR1 musi jednak ustanowić jeden canonical
V1 envelope:

```text
availability = available | unavailable | not_configured
measurement_quality = measured | degraded | insufficient | stale | fallback | legacy_default
authority_class = authoritative | equivalent_cutover | counterfactual | evidence_only | logging_only | export_only
policy_actionable = true | false
reason_codes = typed per contract
```

Każdy legacy enum/string musi mieć exhaustive adapter; unknown mapping ma
failować test/deserialize contract, nie silently mapować do clean. Actionability
wynika równocześnie z availability, quality i authority profile — sama obecność
wartości nie wystarcza.

Klasyfikacja canonical envelope/profile: `MISSING`.

## 7. Schema, logger i replay baseline

Potwierdzone wersje:

| Powierzchnia | Wersja | Dowód |
| --- | ---: | --- |
| Gatekeeper decision JSONL | 33 | `ghost-brain/src/oracle/decision_logger.rs:99` |
| V3 shadow record | 1 | `ghost-launcher/src/components/gatekeeper_v3.rs:13` |
| V3 replay payload | 1 | `ghost-launcher/src/oracle_runtime.rs:142` |
| Gatekeeper V2 replay input | 1 | `decision_logger.rs:2395` i historyczne rows |

DecisionLogger ma bounded mpsc channel o domyślnej pojemności 1000
(`decision_logger.rs:2750-2824`) i awaited sends (`decision_logger.rs:3055-3124`).
Po ENOSPC writer może wyłączyć dalsze file writes (`decision_logger.rs:2831-2840`).
Istnieje niezależny export-only coordination-risk sidecar, ale nie istnieje
jeszcze paired metric-contract summary/sidecar, jego join completeness,
drop/failure counters ani resource acceptance wymagane przez V1.1.

Wniosek dla PR2C: v34 musi być addytywny i compact; pełne typed payloady mają
trafić do osobnego `metric_contract_evidence_v1.jsonl`, spiętego canonical
`(run_id, join_key, decision_plane)` oraz hash/ref. Frozen v33 i V3 v1 muszą
pozostać replayable.

## 8. Active, compat, shadow, logging i export classification

| Powierzchnia | Klasa aktualna |
| --- | --- |
| MFS TxIntel FTDI/top3/same-ms/dev first-observed/legacy FSC | active decision evidence; część pól wpływa na V2 policy |
| GatekeeperBuffer primary creator buy | compatibility/accumulator; nie aktywny Phase 5 authority |
| FSC v2 | shadow/evidence, non-authoritative |
| V3 evidence/manipulation | shadow decision path z frozen v1 replay contract |
| recent buy/sell RCE | `LOGGING_ONLY` |
| coordination FTDI/HHI sidecar | `EXPORT_ONLY` |
| reserve velocity | materialized evidence; brak promocji do aktywnej V2 policy |
| flip V2 | `MISSING`; przyszły `EvidenceOnly` |

## 9. Implementation handoff

### PR1

- dodać registry `METRIC_CONTRACTS_V1_1`, rollout ceiling i hashowany Profile A;
- dodać canonical envelope i exhaustive legacy adapters;
- rozdzielić dev surfaces w registry;
- zinwentaryzować top3 readers i dodać static guard bez duplikowania helpera;
- dodać serde-compatible schema/config types, ale nie aktywować v34 ani dual
  compute.

### PR2A

- FTDI typed value + osobne legacy/corrected actionability;
- first-observed i primary dev evidence, przy zachowaniu first-observed policy
  authority;
- trzy typed same-ms contracts;
- top3 mismatch telemetry;
- typed legacy FSC i rozdzielone `fsc_legacy/fsc_v2`.

### PR2B

- normatywny flip V2 state machine;
- presence-aware manipulation numerics/flags;
- reserve velocity interval/validity evidence;
- recent counts/optional ratio/bounded share;
- bounded dedupe/order/reconnect diagnostics.

### PR2C

- compact v34 i paired full sidecar;
- comparator/replay v2/audit CLI;
- manifest/rotation/SHA/queue/resource gates;
- ponowny historical feasibility audit po istnieniu V2 producers;
- zamrożenie `BURN_IN_CONTRACT_V1` przed jakimikolwiek prospective validation
  rows.

## 10. Testy wykonane na baseline

Wszystkie poniższe polecenia zakończyły się kodem 0:

```text
cargo test -p ghost-launcher ftdi_two_buy_sample_exports_degraded_diagnostic_value
  1 passed

cargo test -p ghost-core --test tx_intelligence_contract_tests
  2 passed

cargo test -p seer test_flip_ratio
  2 passed

cargo test -p ghost-launcher canonical_creator_dev_buy
  3 passed

cargo test -p ghost-launcher --test session_lifecycle_tests materialize_features_populates_
  7 passed

cargo test -p ghost-core --test account_state_core_tests \
  reducer_preserves_raw_reserves_but_exposes_normalized_feature_units
  1 passed

cargo test -p ghost-launcher --test gatekeeper_policy_tests top3_
  3 passed
```

Build emituje zastane warnings, ale nie wystąpił test failure. Pierwsze
uruchomienie FTDI z `--exact` użyło niepełnej nazwy modułowej i wybrało zero
testów; nie jest liczone jako dowód. Powtórzenie bez `--exact` uruchomiło właściwy
test i przeszło.

## 11. Acceptance i granice PASS

Spełnione:

- dziesięć kontraktów ma requirement/current producer/MFS/consumer/schema/status
  i implementation delta;
- aktywny dev Phase 5 source jest jednoznaczny;
- top3 existing helper został zachowany;
- FTDI mismatch jest odtworzony z kodu i testu;
- FSC/status/manipulation/reserve/RCE boundaries są jawne;
- schema/logger/replay baseline jest jawny;
- historyczne runy i ich ograniczenia opisuje osobny preflight;
- nie dokonano zmian runtime/policy/config.

Niespełnione celowo na etapie PR0:

- typed contracts/profile/envelope;
- V2 producers;
- v34/sidecar/comparator;
- pełny replay bundle;
- zamrożony burn-in contract;
- prospective validation;
- PR3 cutover.

Końcowy status:

```text
BASELINE_RECONCILIATION_PASS
```

PASS odblokowuje wyłącznie PR1. Nie stanowi zgody na PR2/PR3 wykonywane poza
kolejnością ani na zmianę active policy.
