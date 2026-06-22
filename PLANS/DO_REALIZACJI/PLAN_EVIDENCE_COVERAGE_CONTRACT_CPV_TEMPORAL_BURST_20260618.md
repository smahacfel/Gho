# Plan wykonawczy: Evidence Coverage Contract dla CPV, temporal carry-forward, delt i burst_ratio

**Data:** 2026-06-18
**Status:** DO REALIZACJI / PLAN PRZED IMPLEMENTACJA
**Tryb:** brak zmian runtime w ramach tego dokumentu
**ADR:** dla samego planu nie tworzyc ADR-8D; dla pozniejszej implementacji ADR-8D jest wymagany zgodnie z repo contract

## 1. Cel

Celem jest uporzadkowanie i wdrozenie wiarygodnego kontraktu evidence dla metryk decyzyjnych Ghosta, tak aby system nie poprawial coverage przez ukryte imputacje, falszywe zera albo zmiane znaczenia metryk.

Plan obejmuje piec zaakceptowanych kierunkow:

1. Zwieszenie coverage CPV dla successful-buy signerow bez zmiany semantyki CPV.
2. Przeniesienie ukrytych progow i polityk do `ghost_brain_config.toml` oraz configow profilowych runow.
3. Dodanie jawnej semantyki temporal carry-forward anchorow: status, source, staleness, carried_from, bez cichego wypelniania.
4. Wyrownanie top-level `burst_ratio` z canonical embedded SSOT, czyli `v3_materialized_feature_snapshot.tx_intel_features.burst_ratio`.
5. Wprowadzenie kontraktu evidence/status/source dla wartosci, delt, rate fields i policy usage.

Najwazniejsza zasada:

```text
coverage zwiekszamy tylko wtedy, gdy nie zmieniamy ukrycie znaczenia metryki
i nie mieszamy braku danych z prawdziwa wartoscia 0.0.
```

## 2. Non-goals

Ten plan nie autoryzuje:

- naprawy FSC coverage lub FSC jako aktywnej metryki polityki;
- wlaczania FSC do CPV combo veto;
- BCV2 exact-watch / account-state hydration workstream;
- zmian live sendera, TX buildera, Helius Sender, execution mode albo post-buy lifecycle;
- strojenia BUY/REJECT thresholdow pod wynik finansowy;
- szerokiego refaktoru Gatekeepera;
- przywrocenia legacy HyperPrediction/Chaos/scoring flow;
- cichej zmiany shadow na live;
- traktowania shadow simulation jako live inclusion;
- datasetowej imputacji `null -> 0`;
- uzycia degraded evidence jako clean evidence.

## 3. Aktualne kotwice w kodzie

Plan opiera sie na aktualnym kodzie, nie na zalozeniach historycznych.

Najwazniejsze miejsca:

- `ghost-brain/src/config/ghost_brain_config.rs:1003` - `GatekeeperV2Config` ma `#[serde(default)]`, wiec nowe pola musza zachowac kompatybilnosc starych configow.
- `ghost-brain/src/config/ghost_brain_config.rs:1265` - istnieje `strict_metric_threshold_gate_enabled`, ale nie ma jawnej polityki missing/degraded.
- `ghost-brain/src/config/ghost_brain_config.rs:1507` - CPV thresholdy polityki istnieja jako `max_signer_cross_pool_velocity` i `min_cpv_other_pool_activity`.
- `ghost-brain/src/config/ghost_brain_config.rs:1587` - CPV rolling-state config obejmuje lookback/per-signer/global cap, ale nie obejmuje progu successful-buy signer sample.
- `ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs:77` - indeks CPV obserwuje tylko successful buys. Ta semantyka ma zostac.
- `ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs:147` - `signer_sample_count` pochodzi z unikalnych successful-buy signerow.
- `ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs:166` - obecne minimum `signer_sample_count < 3` jest hardcoded.
- `ghost-launcher/src/session/observation.rs:955` - CPV jest materializowane do `MaterializedFeatureSet.sybil_resistance`.
- `ghost-launcher/src/session/observation.rs:1005` - temporal CPV deltas sa doklejane po materializacji temporal deltas.
- `ghost-core/src/checkpoint/types.rs:117` - istnieje ogolny `EvidenceStatus`.
- `ghost-core/src/checkpoint/types.rs:136` - istnieja `EvidenceDegradedReason`, w tym `CpvEvidencePartial`, `DecisionTimeSeriesPriceCarriedForward`, `TemporalDeltaAnchorIncomplete`.
- `ghost-core/src/checkpoint/types.rs:485` - `TemporalAnchorSnapshot` ma wartosci anchorow, ale nie ma jeszcze `reached_by`, per-metric source/status ani staleness.
- `ghost-core/src/checkpoint/types.rs:517` - `TemporalDeltaFeatures` ma delty/rate fields, ale bez per-delta evidence status.
- `ghost-core/src/checkpoint/types.rs:669` - `MaterializedFeatureSet` jest kanonicznym snapshotem decyzji.
- `ghost-launcher/src/components/gatekeeper.rs:2589` - top-level `burst_ratio` jest obecnie brany z `phase2_velocity`, a nie z canonical embedded `tx_intel_features`.
- `ghost-launcher/src/components/gatekeeper.rs:6978` - temporal anchor snapshot liczy wartosci anchorow z probek do anchor_ms.
- `ghost-launcher/src/components/gatekeeper.rs:7131` - `reached` jest oparte o `observed_end_event_ts_ms`, co nie odroznia ciszy rynkowej od braku dotarcia obserwacji do anchoru.
- `ghost-launcher/src/components/gatekeeper.rs:7201` - rate fields sa liczone z delt, ale bez statusu evidence.
- `ghost-launcher/src/components/gatekeeper_policy.rs:1117` - strict metric threshold policy ma obecnie missing => hard fail, gdy gate jest wlaczony.
- `ghost-launcher/src/components/gatekeeper_policy.rs:1144` - strict `burst_ratio` opiera sie na `phase2_velocity`.
- `ghost-launcher/src/components/gatekeeper_policy.rs:1165` - strict `flipper_presence_ratio` ma hardcoded missing hard fail.
- `ghost-launcher/src/components/gatekeeper_policy.rs:1192` - strict `jito_tip_intensity` ma hardcoded missing hard fail.
- `ghost-launcher/src/components/gatekeeper_policy.rs:1219` - strict `signer_cross_pool_velocity` ma hardcoded missing hard fail.
- `ghost-launcher/src/components/gatekeeper_policy.rs:1236` - `cpv_other_pool_activity` missing hard-failuje, gdy `min_cpv_other_pool_activity > 0.0`.
- `ghost-brain/src/oracle/decision_logger.rs:2030` - top-level delta/rate fields istnieja jako nullable fields.
- `ghost-launcher/src/components/gatekeeper.rs:3177` - top-level delty sa mapowane z embedded temporal deltas.

## 4. Glowny kontrakt prawdy

Nie wolno mieszac trzech stanow:

| Stan | Znaczenie | Dopuszczalne uzycie |
| --- | --- | --- |
| liczba, w tym `0.0` | metryka zostala policzona i ma taka wartosc | moze byc uzyta zgodnie z policy i statusem |
| `null` / brak pola | nie ma wystarczajacego evidence do policzenia | nie wolno traktowac jako zero |
| `carried_forward` | brak nowego eventu w anchorze, ale znany stan zostal jawnie przeniesiony z przeszlosci | moze byc uzyty tylko z source/status/staleness i zgodnie z config policy |

Twarde zasady:

```text
NAKAZ: kazda wartosc krytyczna musi miec mozliwy do odtworzenia evidence status.
NAKAZ: kazda wartosc carried-forward musi miec source, carried_from_anchor_ms i staleness_ms.
NAKAZ: kazdy policy usage degraded/carried musi byc jawnie dozwolony configiem.
ZAKAZ: null -> 0.0 bez statusu.
ZAKAZ: future-fill, czyli cofanie wartosci z przyszlego eventu do wczesniejszego anchoru.
ZAKAZ: liczenie CPV ze wszystkich signerow, failed tx albo sell-only wallets.
ZAKAZ: top-level field z innej semantyki niz embedded canonical SSOT.
ZAKAZ: zmiana defaultow w sposob zmieniajacy historyczne decyzje bez jawnego rollout profilu.
```

## 5. Proponowane nazewnictwo configu

Minimalny zestaw wymagany w `GatekeeperV2Config` i `ghost_brain_config.toml`.

Rekomendacja implementacyjna: zaczac od plaskich pol w `[gatekeeper_v2]`, bo `GatekeeperV2Config` jest juz duzy, aktywny i ma `#[serde(default)]`. Plaskie pola minimalizuja ryzyko migracji TOML. Jesli implementer udowodni, ze istniejacy loader stabilnie obsluguje nested podstruktury, mozna pozniej opakowac je w podsekcje, ale nie jest to wymagane w pierwszej implementacji.

```toml
[gatekeeper_v2]

# Strict metric policy.
strict_metric_missing_policy = "hard_fail" # "hard_fail" | "skip" | "degraded_allowed"

# CPV successful-buy signer sample policy.
cpv_low_sample_policy = "hard_fail" # "hard_fail" | "use_degraded" | "reason_only"
cpv_min_successful_buy_signers_clean = 3
cpv_min_successful_buy_signers_degraded = 2
cpv_emit_degraded_low_sample = false
cpv_allow_degraded_in_strict_policy = false

# Temporal carry-forward.
temporal_carried_forward_policy = "log_only" # "use_for_selector_only" | "use_in_policy" | "log_only"
temporal_carry_forward_enabled = false
temporal_carry_forward_max_staleness_ms = 1000
temporal_carry_forward_event_counters_enabled = true
temporal_carry_forward_state_metrics_enabled = false
temporal_carry_forward_ratio_metrics_enabled = false

# Logging / replay parity.
top_level_features_from_materialized_ssot = true
emit_evidence_policy_context = true
```

Wymagane enumy:

```rust
StrictMetricMissingPolicy:
  hard_fail
  skip
  degraded_allowed

CpvLowSamplePolicy:
  hard_fail
  use_degraded
  reason_only

TemporalCarriedForwardPolicy:
  log_only
  use_for_selector_only
  use_in_policy
```

Wszystkie enumy:

- `#[serde(rename_all = "snake_case")]`;
- `Default` zgodny z historycznym fail-closed zachowaniem;
- parsowanie starego configu bez nowych pol musi dzialac;
- nieznana wartosc w TOML ma failowac jasno, nie przechodzic cicho.

### 5.1 Domyslne wartosci bez regresji

Defaulty bazowe maja zachowac dotychczasowa semantyke:

```text
strict_metric_missing_policy = hard_fail
cpv_low_sample_policy = hard_fail
cpv_min_successful_buy_signers_clean = 3
cpv_emit_degraded_low_sample = false
temporal_carried_forward_policy = log_only
temporal_carry_forward_enabled = false
```

To oznacza:

- stare configi nie zaczna nagle uzywac degraded CPV;
- temporal carry-forward nie zacznie zmieniac delt bez jawnego profilu;
- strict metric missing pozostanie fail-closed tam, gdzie strict gate jest wlaczony;
- nowe profile R37/R38 moga jawnie wlaczyc wieksze coverage.

## 6. PR/Task breakdown

Plan nalezy realizowac jako maksymalnie 5 oddzielnych PR/taskow. Nie wolno wrzucic CPV, carry-forward, policy i logging parity w jeden patch.

### PR1 - Config i Evidence Foundation

**Cel:** dodac jawne polityki i typy evidence bez zmiany aktywnego zachowania Gatekeepera.

Zakres:

- dodac enumy configowe:
  - `StrictMetricMissingPolicy`;
  - `CpvLowSamplePolicy`;
  - `TemporalCarriedForwardPolicy`;
- dodac pola configowe do `GatekeeperV2Config`;
- dodac defaulty zachowujace obecna semantyke;
- dodac pola do `ghost-brain/ghost_brain_config.toml`;
- dodac te same pola do aktywnych configow profilowych rolloutow, ktore beda uzywane do testu R37/R38;
- przygotowac additive typy evidence dla CPV/temporal, ale bez policy usage.

Rekomendowane typy domenowe:

```rust
TemporalAnchorReachedBy:
  event
  observation_elapsed
  deadline
  not_reached

TemporalMetricSource:
  observed
  carried_forward_no_event
  unavailable
  not_configured

MetricEvidenceQuality:
  clean
  degraded_low_sample
  carried_forward
  insufficient_sample
  unavailable
```

Nie trzeba duplikowac `EvidenceStatus`, jesli da sie go wykorzystac. Dodatkowe enumy maja doprecyzowac source/status na poziomie metryki, bo ogolny `EvidenceStatus` nie wystarcza do rozroznienia np. `0 observed` vs `0 carried_forward`.

NAKAZY:

- `serde(default)` dla kazdego nowego pola configowego.
- `rename_all = "snake_case"` dla enumow.
- Test deserializacji starego configu bez nowych pol.
- Test deserializacji configu z kazda wartoscia enum.
- Logowac policy context w decyzji albo embedded replay context, jezeli policy moze zmienic interpretacje pola.
- Zmiany schema/logging tylko additive.

ZAKAZY:

- Nie zmieniac strict policy behavior w PR1.
- Nie wlaczac carry-forward.
- Nie obnizac CPV sample threshold w runtime.
- Nie zmieniac BUY/REJECT/TIMEOUT.
- Nie zmieniac execution/shadow behavior.

Potencjalne regresje i jak ich uniknac:

| Regresja | Kiedy nastapi | Unikniecie |
| --- | --- | --- |
| Stare configi przestaja sie ladowac | nowe pola bez defaultow | `#[serde(default)]`, test old-config load |
| Dwa runy sa nieporownywalne | policy values nie sa logowane | `emit_evidence_policy_context`, config hash, decision context |
| Degraded zaczyna znaczyc clean | enumy sa zbyt ogolne | osobne source/status i jasna tabela policy usage |
| Runtime policy zmienia sie w PR1 | config jest od razu konsumowany przez Gatekeepera | PR1 tylko foundation i tests |

DoD:

- `cargo test` dla config/serde/unitow zwiazanych z enumami przechodzi.
- Stary minimalny `GatekeeperV2Config` laduje sie bez nowych pol.
- Nowe pola sa obecne w `ghost_brain_config.toml` i aktywnych profilach runow.
- Defaulty w kodzie i TOML sa spojne.
- Decyzje z defaultowym configiem nie zmieniaja policy path.

### PR2 - CPV Successful-Buy Coverage Contract

**Cel:** zwiekszyc coverage CPV tylko w ramach successful-buy signer semantics i jawnego evidence statusu.

Aktualny problem:

- CPV uzywa dobrego denominatora: unique successful-buy signers.
- Minimalny prog `3` jest hardcoded.
- Przy `signer_sample_count < 3` wartosc CPV jest `None`, a powody sa tylko w `degraded_reasons`.
- Nie ma rozroznienia: low sample vs rolling index unavailable vs config disabled.

Zakres:

- rozszerzyc `CrossPoolVelocityConfig` o:
  - `min_successful_buy_signers_clean`;
  - `min_successful_buy_signers_degraded`;
  - `emit_degraded_low_sample`;
- zmapowac te wartosci z `GatekeeperV2Config`;
- rozszerzyc `CpvComputation` o:
  - `status`;
  - `sample_count`;
  - `required_clean_sample_count`;
  - `required_degraded_sample_count`;
  - `value_source`;
  - `degraded_reason`;
  - `rolling_state_available`;
- utrzymac `unique_successful_signers(transactions)` jako jedyny denominator;
- w `MaterializedFeatureSet.sybil_resistance` albo `evidence_status.cpv` zapisac CPV evidence;
- dla low sample pozwolic policzyc wartosc tylko wtedy, gdy `cpv_emit_degraded_low_sample = true` i `sample_count >= min_successful_buy_signers_degraded`;
- nie uzywac degraded CPV w strict policy, dopoki PR5 nie wprowadzi jawnego policy wiring.

Semantyka:

```text
sample_count >= clean_min
  => value Some, status clean

degraded_min <= sample_count < clean_min i emit_degraded_low_sample=true
  => value Some, status degraded_low_sample

sample_count < degraded_min
  => value None, status insufficient_sample

rolling index unavailable
  => value None, status unavailable_source
```

NAKAZY:

- CPV nadal liczy tylko successful-buy signerow.
- `failed tx`, `sell-only wallets`, wszyscy signerzy i unikalne adresy ogolem sa zabronione jako denominator CPV.
- `cpv_other_pool_activity` musi dostac taki sam evidence status/sample metadata jak `signer_cross_pool_velocity`.
- Kazda wartosc low-sample musi miec status `degraded_low_sample`.
- Reason musi odrozniac:
  - `CPV_INSUFFICIENT_SUCCESSFUL_BUY_SIGNERS`;
  - `CPV_ROLLING_STATE_UNAVAILABLE`;
  - `CPV_DISABLED_BY_CONFIG`;
  - `CPV_LOW_SAMPLE_DEGRADED`.

ZAKAZY:

- Nie podmieniac denominatora na `unique_signers_evaluated`.
- Nie emitowac low-sample CPV jako clean.
- Nie ukrywac sample_count.
- Nie zmieniac defaultowego progu clean `3`.
- Nie wlaczac degraded CPV do Gatekeeper strict policy w tym PR.

Potencjalne regresje i jak ich uniknac:

| Regresja | Kiedy nastapi | Unikniecie |
| --- | --- | --- |
| CPV traci znaczenie | ktos liczy wszystkich signerow zamiast successful buys | testy denominatora i code review na `unique_successful_signers` |
| Coverage rosnie falszywie | low sample dostaje status clean | status `degraded_low_sample`, required counts w logu |
| Strict policy zaczyna przepuszczac slabe CPV | PR2 podlaczy degraded do policy | policy wiring dopiero w PR5 |
| CPV missing myli sie z index unavailable | jeden reason dla wszystkiego | rozdzielone reason/status |
| Lock contention wzrasta | dodatkowe obliczenia pod write lockiem CPV | nie robic IO/logowania pod lockiem, utrzymac bounded history |

DoD:

- Test: 1 successful-buy signer => CPV value `None`, status `insufficient_sample`.
- Test: 2 successful-buy signerow przy `emit_degraded_low_sample=false` => value `None`, status `insufficient_sample` albo `degraded_not_emitted`, bez policy use.
- Test: 2 successful-buy signerow przy `emit_degraded_low_sample=true` => value `Some`, status `degraded_low_sample`, sample_count=2, required_clean=3.
- Test: 3 successful-buy signerow => value `Some`, status `clean`.
- Test: failed buy, failed sell, sell-only signer nie zwiekszaja CPV sample.
- Runtime log zawiera sample_count/status/reason dla CPV.
- Przy default configu zachowanie BUY/REJECT jest niezmienione.

### PR3 - Temporal Carry-Forward Anchor Semantics

**Cel:** wprowadzic jawne carry-forward dla temporal anchors i delt, bez cichej imputacji i bez future-fill.

Aktualny problem:

- `anchor.reached` jest strict-anchor i zalezy od `observed_end_event_ts_ms`.
- Jesli system realnie czekal do 3s, ale po 2s nie bylo eventu, `1s_to_3s` moze pozostac `null`.
- Dla licznikow eventowych cisza po 2s moze oznaczac znany brak zmiany, ale obecny format nie pozwala tego zadeklarowac.
- Dla price/state/ratio cisza nie zawsze oznacza prawdziwa stabilnosc, wiec carry-forward musi miec source/staleness.

Zakres:

- dodac do anchorow metadata:
  - `reached_by`;
  - `anchor_observation_elapsed_ms`;
  - `value_source` albo per-metric sources;
  - `carried_from_anchor_ms`;
  - `staleness_ms`;
  - `status`;
- rozdzielic event-time od observation-elapsed:
  - event-time okresla kolejke faktow rynkowych;
  - observation elapsed/deadline okresla, czy runtime dotarl do anchoru mimo ciszy;
- dodac carry-forward tylko z przeszlosci do pozniejszego anchoru;
- dodac per-delta evidence status:
  - `observed`;
  - `carried_forward_no_event`;
  - `partial_carried_forward`;
  - `unavailable`;
  - `insufficient_sample`;
- objac rate fields tym samym statusem co delta bazowa.

Klasy metryk:

| Klasa | Metryki | Carry-forward domyslnie | Uzasadnienie |
| --- | --- | --- | --- |
| Event counters | `tx_count`, `buy_count`, `unique_signers`, `net_quote_sol`, `total_volume_sol` | dozwolone, gdy obserwacja dotarla do anchoru | brak eventu oznacza brak zmiany licznika w obserwowanym strumieniu |
| State/price | `price`, `market_cap`, account-state fields | niedozwolone domyslnie | brak tx nie dowodzi braku account update |
| Ratios | `burst_ratio`, `jito_tip_intensity`, `flipper_presence_ratio`, CPV | niedozwolone domyslnie albo log-only | ratio bez nowego eventu moze byc matematycznie stale, ale musi byc jawnie carried |

Wymagany przyklad zachowania:

```text
1s: net_quote_sol = 2.0, observed
2s: net_quote_sol = -0.02, observed
3s: brak tx, ale obserwacja runtime dotarla do 3s

delta_net_quote_sol_1s_to_2s = -2.02
delta_net_quote_sol_1s_to_2s_status = observed

delta_net_quote_sol_1s_to_3s = -2.02
delta_net_quote_sol_1s_to_3s_status = carried_forward_no_event
delta_net_quote_sol_1s_to_3s_carried_from_anchor_ms = 2000
delta_net_quote_sol_1s_to_3s_staleness_ms = 1000

rate_net_quote_sol_per_s_1s_to_3s = -1.01
rate_net_quote_sol_per_s_1s_to_3s_status = carried_forward_no_event
```

No future-fill:

```text
1s: unknown
2s: unknown
3s: first observed value

Zakaz:
  anchor_1s = value z 3s
  anchor_2s = value z 3s

Poprawnie:
  anchor_1s value null, status unavailable
  anchor_2s value null, status unavailable
  anchor_3s value observed
```

NAKAZY:

- Carry-forward tylko wtedy, gdy runtime dotarl do anchoru (`observation_elapsed` albo `deadline`), a ostatnia znana wartosc pochodzi z przeszlosci.
- Kazda carried wartosc musi miec `carried_from_anchor_ms` i `staleness_ms`.
- `staleness_ms <= temporal_carry_forward_max_staleness_ms`.
- Dla state/price carry-forward domyslnie off.
- Dla ratio carry-forward domyslnie off albo log-only, dopoki nie bedzie osobnej walidacji.
- Rate field dziedziczy status delty.
- Top-level/dataset musi miec sposob odtworzenia statusu, nawet jesli nie wszystkie statusy sa splaszczone.

ZAKAZY:

- Nie generowac syntetycznego eventu na 3s.
- Nie zmieniac timestampow tickow.
- Nie cofac wartosci z pozniejszego eventu.
- Nie wpisywac `0` dla delty, gdy anchor jest unknown.
- Nie traktowac `carried_forward_no_event` jako clean.
- Nie wlaczac `use_in_policy` w pierwszym runtime profilu.

Potencjalne regresje i jak ich uniknac:

| Regresja | Kiedy nastapi | Unikniecie |
| --- | --- | --- |
| Model uczy sie runtime ciszy jako rynku | carry-forward bez source/status | explicit `carried_forward_no_event` i staleness |
| False stability | state/price carry-forward domyslnie wlaczony | state/price off by default |
| Future leakage | wartosc z 3s wypelnia 1s/2s | test no-future-fill |
| Anchor reached jest falszywy | event-time mylony z wall/deadline | osobne `reached_by` i `observation_elapsed` |
| Policy zaczyna uzywac carried fields | `use_in_policy` wlaczone zbyt wczesnie | profile startuja od `log_only` albo `use_for_selector_only` |
| Replay drift | carry-forward zalezy od live wall-clock bez logu | logowac `reached_by`, elapsed, deadline/context |

DoD:

- Test no-event 2s->3s dla event counters daje delta liczbe + status `carried_forward_no_event`.
- Test no-event 2s->3s dla price/state przy default configu zostawia delta `null` i status `unavailable` albo `not_allowed`.
- Test ratio carry-forward przy default configu nie tworzy clean ratio.
- Test future-fill: first value after anchor nie wypelnia wczesniejszych anchorow.
- Test `max_staleness_ms`: przekroczony staleness => `null`, reason `stale`.
- Embedded `TemporalDeltaFeatures` pozwala odtworzyc source/status kazdej carried delty.
- Top-level fields nie ukrywaja statusu, jezeli value jest emitted.

### PR4 - DecisionLogger, top-level parity i burst_ratio Option A

**Cel:** doprowadzic logi i top-level convenience fields do zgodnosci z embedded canonical SSOT bez utraty evidence.

Zakres:

- ustawic top-level `burst_ratio` jako canonical:

```text
top-level burst_ratio == v3_materialized_feature_snapshot.tx_intel_features.burst_ratio
```

- jesli stara `phase2_velocity.burst_ratio` nadal jest potrzebna diagnostycznie, dodac osobne pole:

```text
phase2_burst_ratio
```

- nie uzywac `phase2_burst_ratio` jako zamiennika canonical `burst_ratio`;
- zapewnic parity top-level delta/rate fields z embedded temporal deltas;
- dopilnowac `rate_mcap_sol_per_s_2s_to_3s` top-level parity, bo pole istnieje i jest mapowane, wiec problem jest walidacyjny/loggingowy, nie domenowy;
- bump schema/log schema version, jesli dodawane sa top-level status/source pola albo zmienia sie semantyka top-level `burst_ratio`;
- logowac config/evidence policy context;
- jezeli top-level `vectors_prices` nadal istnieje jako convenience vector, musi zachowac nullable shape i dlugosc embedded `decision_time_series.prices`.

NAKAZY:

- Top-level fields sa convenience projection z embedded SSOT, nie osobnym zrodlem prawdy.
- Embedded `v3_materialized_feature_snapshot` pozostaje pelnym evidence.
- Schema version/config hash ma pozwolic odroznic stary run od nowego.
- `null` w nullable vectorze ma pozostac `null`, nie usuwac calego pola i nie skracac wektora.
- Dataset builder ma miec jednoznaczna sciezke do status/source.

ZAKAZY:

- Nie liczyc top-level `burst_ratio` ponownie z raw tx.
- Nie brac top-level `burst_ratio` z `phase2_velocity`, skoro user wybral Option A.
- Nie emitowac top-level wartosci bez mozliwosci odtworzenia statusu.
- Nie zmieniac shadow execution semantics przy okazji loggera.
- Nie zmieniac verdictow przez zmiany w DecisionLogger.

Potencjalne regresje i jak ich uniknac:

| Regresja | Kiedy nastapi | Unikniecie |
| --- | --- | --- |
| Offline selector trenuje na innej wartosci niz Gatekeeper evidence | top-level i embedded maja inne zrodla | projection only from MaterializedFeatureSet |
| TIMEOUT rows wygladaja jak missing burst artifact | top-level dalej phase2-only | Option A, canonical `tx_intel_features.burst_ratio` |
| Replay nie odroznia starego schema shape | brak schema bump | bump schema/log version i config context |
| DTW traci indeksy cen | nullable vector z `null` zostaje pominiety | zachowac shape array z nullami |
| Rate field znika mimo embedded value | serializer lub mapping pomija field | test top-level vs embedded dla rate/delta |

DoD:

- Test: top-level `burst_ratio` rowna sie embedded canonical `tx_intel_features.burst_ratio`.
- Test: przy `<2 TX`, jezeli embedded canonical ma fallback/wartosc, top-level ma ta sama wartosc i status/context.
- Test: `phase2_burst_ratio`, jesli dodany, nie nadpisuje `burst_ratio`.
- Test: wszystkie top-level delty zadania rownaja sie embedded temporal deltas.
- Test: `rate_mcap_sol_per_s_2s_to_3s` top-level pojawia sie, gdy embedded ma wartosc.
- Test: nullable `vectors_prices` zachowuje dlugosc embedded `prices`, lacznie z `null`.
- JSONL validator rozdziela `missing`, `degraded`, `carried_forward`, `observed`.

### PR5 - Policy wiring, reason codes i rollout validation

**Cel:** dopiero po ustabilizowaniu evidence/config/logging podlaczyc polityki do Gatekeepera i potwierdzic runtime bez regresji shadow/simulation.

Zakres:

- podlaczyc `strict_metric_missing_policy` w `strict_metric_threshold_failure_from_assessment`;
- podlaczyc `cpv_low_sample_policy` dla CPV strict metrics;
- podlaczyc `temporal_carried_forward_policy` tylko dla miejsc, ktore faktycznie konsumuje selector/policy;
- dodac reason chain rozrozniajacy:
  - metric value threshold failure;
  - metric missing evidence;
  - degraded evidence not allowed;
  - carried-forward not allowed;
  - low sample reason-only;
- nie traktowac degraded/carry jako clean;
- utrzymac defaulty fail-closed;
- rollout zaczac od shadow/log-only profilu.

Semantyka policy:

#### `strict_metric_missing_policy`

```text
hard_fail:
  unknown/null critical metric => HARD_FAIL with reason strict_metric_missing=<metric>

skip:
  unknown/null critical metric => metric skipped, reason metric_skipped_missing=<metric>
  Nie wolno traktowac jako pass.

degraded_allowed:
  numeric value with degraded/carried status can be evaluated only when its own policy allows it.
  Pure null/unavailable remains not-a-number and cannot become 0.
```

#### `cpv_low_sample_policy`

```text
hard_fail:
  CPV low sample => strict policy rejects if CPV required

reason_only:
  CPV low sample evidence is logged, but strict CPV value is not used as threshold pass

use_degraded:
  CPV degraded low sample value can be evaluated by threshold,
  but reason/status must remain degraded_low_sample
```

#### `temporal_carried_forward_policy`

```text
log_only:
  compute/log carried evidence, no selector/policy usage

use_for_selector_only:
  expose carried evidence to dataset/selector features with status,
  Gatekeeper hard policy does not consume it

use_in_policy:
  Gatekeeper may consume carried fields only for allowlisted metrics and statuses
```

NAKAZY:

- Reason codes musza rozdzielac `bad metric value` od `missing metric evidence`.
- Policy ma sprawdzac status przed uzyciem wartosci degraded/carried.
- `use_in_policy` musi byc allowlistowane per metric class, nie globalne dla wszystkiego.
- Shadow-only run musi pokazac policy context w decyzjach.
- Replay/offline validator musi widziec te same statusy co runtime.

ZAKAZY:

- Nie przepuszczac `degraded_low_sample` CPV jako clean.
- Nie pozwalac `degraded_allowed` na zamiane null w zero.
- Nie zmieniac policy dla live bez osobnego zatwierdzenia.
- Nie mieszac missing-evidence rejects z threshold-value rejects w jednym reason bucket.
- Nie uzywac temporal carried-forward do hard BUY path w pierwszym rollout profilu.

Potencjalne regresje i jak ich uniknac:

| Regresja | Kiedy nastapi | Unikniecie |
| --- | --- | --- |
| Gatekeeper odrzuca dobry token z powodu runtime coverage | missing evidence i bad value maja ten sam reason | osobne reason buckets i raport coverage |
| Gatekeeper przepuszcza slaby token na degraded CPV | `use_degraded` wlaczone bez status/policy check | allowlist + status check + default hard_fail |
| Selector uczy sie artefaktu runtime | dataset widzi liczbe bez statusu | value + present + status + source + staleness |
| Shadow simulation zmienia sie przy logger change | PR dotyka execution/shadow | no execution files poza logging context, smoke shadow-only |
| Replay drift | config policy nie jest w payloadzie | config hash + policy context w JSONL |
| Strict policy staje sie skip-by-default | zly default enum | default `hard_fail`, test old behavior |

DoD:

- Unit tests dla kazdej wartosci `strict_metric_missing_policy`.
- Unit tests dla kazdej wartosci `cpv_low_sample_policy`.
- Unit tests dla kazdej wartosci `temporal_carried_forward_policy`.
- Test: missing jito/flipper/CPV przy `hard_fail` nadal hard-failuje.
- Test: missing przy `skip` nie jest pass, tylko reason `metric_skipped_missing`.
- Test: degraded CPV przy `reason_only` jest logowane, ale nie zalicza threshold pass.
- Test: degraded CPV przy `use_degraded` jest threshold-evaluated i nadal ma status degraded.
- Test: carried temporal value przy `log_only` nie wplywa na Gatekeeper verdict.
- Test: carried temporal value przy `use_for_selector_only` jest eksportowane z evidence, ale nie zmienia Gatekeeper policy.
- Test: `use_in_policy` dziala tylko dla allowlistowanych metryk.
- Shadow smoke: brak zmian live/shadow boundary, brak live inclusion, brak execution side effect.

## 7. Minimalne profile rolloutowe

Implementacja powinna przygotowac co najmniej trzy profile configowe albo trzy jawne zestawy parametrow dla runow.

### 7.1 Baseline compatibility

Cel: udowodnic brak regresji historycznej.

```toml
strict_metric_missing_policy = "hard_fail"
cpv_low_sample_policy = "hard_fail"
cpv_min_successful_buy_signers_clean = 3
cpv_emit_degraded_low_sample = false
temporal_carried_forward_policy = "log_only"
temporal_carry_forward_enabled = false
top_level_features_from_materialized_ssot = true
emit_evidence_policy_context = true
```

Oczekiwanie:

- policy behavior bez zmian;
- nowe pola evidence/logging obecne;
- top-level `burst_ratio` parity z embedded canonical;
- brak uzycia carry-forward.

### 7.2 Evidence expansion / shadow log-only

Cel: zobaczyc coverage bez wplywu na Gatekeeper policy.

```toml
strict_metric_missing_policy = "hard_fail"
cpv_low_sample_policy = "reason_only"
cpv_min_successful_buy_signers_clean = 3
cpv_min_successful_buy_signers_degraded = 2
cpv_emit_degraded_low_sample = true
cpv_allow_degraded_in_strict_policy = false
temporal_carried_forward_policy = "log_only"
temporal_carry_forward_enabled = true
temporal_carry_forward_event_counters_enabled = true
temporal_carry_forward_state_metrics_enabled = false
temporal_carry_forward_ratio_metrics_enabled = false
```

Oczekiwanie:

- CPV coverage moze wzrosnac przez degraded low sample, ale nie zmienia verdictow;
- temporal carry-forward event counters sa widoczne jako evidence, ale nie sa policy input;
- dataset moze mierzyc wartosc coverage bez ryzyka live/policy.

### 7.3 Selector-only evidence

Cel: dac selektorowi wartosci + statusy bez hard-policy use.

```toml
strict_metric_missing_policy = "hard_fail"
cpv_low_sample_policy = "reason_only"
cpv_emit_degraded_low_sample = true
temporal_carried_forward_policy = "use_for_selector_only"
temporal_carry_forward_enabled = true
temporal_carry_forward_event_counters_enabled = true
temporal_carry_forward_state_metrics_enabled = false
temporal_carry_forward_ratio_metrics_enabled = false
```

Oczekiwanie:

- selector/dataset moze dostac carried event counters;
- Gatekeeper hard policy nie konsumuje carried temporal fields;
- status/source/staleness sa dostepne w dataset builderze.

`use_in_policy` jest poza pierwszym bezpiecznym rolloutem. Moze byc dopuszczone dopiero po osobnym audycie artifactow.

## 8. Acceptance gates runtime

Nie zamykac implementacji bez runtime artifact proof.

### 8.1 JSONL / DecisionLogger gates

Na swiezym shadow runie:

- `v3_materialized_feature_snapshot` obecny w 100% rekordow decyzyjnych.
- Top-level `burst_ratio` == embedded `tx_intel_features.burst_ratio` dla 100% rekordow, w ktorych embedded field istnieje.
- Jesli `phase2_burst_ratio` istnieje, jest osobnym polem i nie nadpisuje `burst_ratio`.
- Top-level delta/rate fields rownaja sie embedded temporal deltas, lacznie z `rate_mcap_sol_per_s_2s_to_3s`.
- Top-level nullable vectors, jesli emitowane, zachowuja dlugosc embedded arrays i nie usuwaja pozycji `null`.
- `log_schema_version` albo rownowazny schema marker odroznia nowy shape od R36/R37.
- JSONL zawiera policy context:
  - `strict_metric_missing_policy`;
  - `cpv_low_sample_policy`;
  - `temporal_carried_forward_policy`;
  - CPV sample thresholds;
  - carry-forward enabled/max staleness.

### 8.2 CPV gates

Na swiezym shadow runie i unitach:

- `cpv_sample_count` liczy only unique successful-buy signers.
- `unique_signers_evaluated` i `cpv_sample_count` moga sie roznic i to jest oczekiwane.
- CPV low-sample rows maja:
  - value absent albo degraded value zgodnie z configiem;
  - `status=degraded_low_sample` albo `insufficient_sample`;
  - `required_clean_sample_count`;
  - `required_degraded_sample_count`;
  - reason.
- Brak recordow, w ktorych CPV value istnieje bez sample_count/status.
- Brak recordow, w ktorych CPV low-sample ma status clean.
- `cpv_other_pool_activity` ma status zgodny z `signer_cross_pool_velocity`.

### 8.3 Temporal carry-forward gates

Na syntetycznych testach i runtime artifactach:

- `series_negative_interval_records == 0` pozostaje utrzymane.
- `delta_* = 0` wystepuje tylko wtedy, gdy oba anchory maja znane wartosci i brak zmiany albo jawny carry-forward status.
- `delta_* = null` oznacza realny brak mozliwosci policzenia, a nie cicha cisze po 2s.
- Kazda carried delta ma:
  - `status=carried_forward_no_event`;
  - `carried_from_anchor_ms`;
  - `staleness_ms`;
  - `reached_by=observation_elapsed` albo `deadline`;
  - brak future-fill.
- `rate_*` ma status zgodny z delta, z ktorej powstal.
- State/price carry-forward nie pojawia sie przy default configu.
- Ratio carry-forward nie pojawia sie jako clean.

### 8.4 Gatekeeper policy gates

Na unitach i shadow runie:

- `strict_metric_missing_policy=hard_fail` zachowuje obecne fail-closed strict behavior.
- `strict_metric_missing_policy=skip` nie robi pass; loguje skip reason.
- `strict_metric_missing_policy=degraded_allowed` nie zamienia null na zero.
- `cpv_low_sample_policy=reason_only` nie zmienia verdictow wzgledem braku CPV w strict policy.
- `cpv_low_sample_policy=use_degraded` moze uzyc degraded value tylko z `cpv_allow_degraded_in_strict_policy=true`.
- `temporal_carried_forward_policy=log_only` nie zmienia Gatekeeper verdictow.
- `temporal_carried_forward_policy=use_for_selector_only` nie zmienia Gatekeeper verdictow.
- `temporal_carried_forward_policy=use_in_policy` jest testowane tylko na allowlistowanych metrykach.

### 8.5 Shadow / simulation gates

Na shadow smoke:

- `entry_mode` i `execution_mode` pozostaja shadow-only dla profilu testowego.
- Brak nowego live submission path.
- Brak zmian w TX builderze.
- Brak zmian w post-buy lifecycle poza ewentualnym decision evidence context.
- Shadow dispatch/lifecycle artifacts maja te same join keys.
- `submit` nadal nie jest traktowany jako confirmation.
- `unknown execution status` nadal nie jest success.

## 9. Regresje krytyczne - pelna lista kontrolna

Ta sekcja ma byc uzyta jako checklist przed mergem kazdego PR.

| Regresja | Mechanizm | Kiedy najbardziej grozi | Wymagana ochrona |
| --- | --- | --- | --- |
| `null` staje sie `0.0` | helper `unwrap_or_default()` albo dataset fillna | PR3/PR4/dataset builder | brak silent default, status wymagany |
| CPV liczy zly denominator | presja na coverage | PR2 | test failed/sell-only/all-signers |
| Low-sample CPV jako clean | value `Some` bez statusu | PR2/PR5 | status `degraded_low_sample`, policy check |
| Carry-forward bez evidence | value przeniesione bez source/staleness | PR3 | per-metric source/status/staleness |
| Future-fill | wartosc pozniejsza wypelnia wczesniejszy anchor | PR3 | test first value after anchor |
| Event-time i wall-time sa pomieszane | syntetyczny timestamp anchoru | PR3 | osobne `reached_by` i observation elapsed |
| Top-level/embedded drift | logger bierze inne zrodlo niz MFS | PR4 | projection from MaterializedFeatureSet |
| Burst ratio ma dwie semantyki | top-level phase2, embedded tx-intel | PR4 | Option A, phase2 osobno |
| Schema ambiguity | nowe top-level fields bez schema bump | PR4 | schema marker/version bump |
| Strict policy skip jako pass | `skip` traktowany jak pozytywny wynik | PR5 | reason `metric_skipped_missing`, no pass |
| Degraded allowed za szeroko | global flag bez allowlisty | PR5 | per metric/status allowlist |
| Shadow/live boundary blur | testowy config wlacza live przez przypadek | PR5 rollout | explicit shadow config gate |
| Replay drift | policy context nie zapisany | PR1/PR4 | config hash i policy context |
| Dataset runtime artifact leakage | selector dostaje value bez statusu | PR4/PR5 | value + present + status + source |
| Lock contention CPV | dodatkowa diagnostyka pod write lockiem | PR2 | bounded compute, no IO/log under lock |
| Broad refactor | laczenie wielu tematow w jeden PR | caly plan | max 5 PR, narrow scope |

## 10. Dataset builder contract

Dataset builder nie moze imputowac po cichu.

Minimalny logiczny format dla krytycznych metryk:

```text
metric_value
metric_present
metric_status
metric_source
metric_sample_count
metric_required_sample_count
metric_staleness_ms
metric_degraded_reason
```

Nie kazde pole musi byc top-level w JSONL, ale dataset builder musi moc je odtworzac z embedded snapshotu.

Reguly:

- `jito_tip_intensity=0.0` oznacza "metryka policzona i brak jito tipow w znanej probie".
- `jito_tip_intensity=null` oznacza "nie ma evidence do policzenia".
- `delta_buy_count=0 observed` oznacza "oba anchory znane i brak zmiany".
- `delta_buy_count=0 carried_forward_no_event` oznacza "drugi anchor przeniesiony przez cisze".
- `CPV missing because 2 successful-buy signers` nie jest tym samym co `CPV missing because rolling index unavailable`.
- `burst_ratio missing top-level` nie moze oznaczac innej prawdy niz embedded canonical.

## 11. Kolejnosc wykonania

Kolejnosc jest obowiazkowa:

```text
PR1 config/evidence foundation
  -> PR2 CPV evidence coverage
  -> PR3 temporal carry-forward semantics
  -> PR4 logger/top-level parity
  -> PR5 policy wiring + rollout validation
```

Nie przeskakiwac do PR5 przed PR1-PR4, bo policy bez evidence contractu tworzy regresje.

Nie mergowac PR2 i PR5 razem, bo wtedy nie da sie odroznic:

- czy coverage wzroslo przez lepsza materializacje;
- czy verdicty zmienily sie przez policy;
- czy logi tylko inaczej wygladaja.

## 12. Minimalne testy i komendy do dobrania przy implementacji

Dokladne komendy nalezy dobrac po aktualnym `Cargo.toml`, ale minimalne klasy testow sa obowiazkowe:

- config serde/default tests;
- CPV unit tests;
- temporal anchor/carry-forward unit tests;
- DecisionLogger serialization tests;
- Gatekeeper strict policy tests;
- replay/JSONL parity validator;
- shadow smoke run dla profilu log-only.

Przy kazdym PR:

```text
git diff --check
cargo test -p <najwezszy-crate> <targeted-tests>
```

Przy PR5 dodatkowo:

```text
fresh shadow run
JSONL audit script
top-level vs embedded parity report
shadow/live boundary report
```

## 13. Kryterium zamkniecia calego watku

Temat mozna uznac za domkniety dopiero, gdy:

1. Wszystkie nowe polityki sa w configu i w profilach runow.
2. Stare configi laduja sie bez zmian.
3. CPV nadal liczy successful-buy signerow, a coverage rosnie tylko przez jawny degraded status.
4. Carry-forward ma source/status/staleness i nie robi future-fill.
5. Top-level `burst_ratio` jest zgodny z embedded canonical tx-intel.
6. Top-level delta/rate fields sa zgodne z embedded temporal deltas.
7. Dataset builder moze rozroznic observed/null/carried/degraded/unavailable.
8. Gatekeeper reason chain rozroznia missing evidence od bad metric value.
9. Shadow simulation i live boundary sa nietkniete.
10. Runtime artifact z nowego runu potwierdza bramki z sekcji 8.

Jesli ktorykolwiek punkt nie przejdzie, nie wolno opisywac implementacji jako "formalnie kompletna". Mozna wtedy zamknac tylko podzakres, np. "core embedded evidence dziala", ale nie caly evidence coverage contract.

## 14. Decyzje zatwierdzone przez usera

- CPV zostaje przy successful-buy signerach.
- Liczenie wszystkich signerow, failed tx albo sell-only wallets jest zabronione.
- Wszystkie progi i polityki maja trafic do configu.
- Carry-forward ma byc jawny, nie cichy.
- `burst_ratio` top-level ma byc Option A: zgodny z embedded canonical tx-intel.
- Model/selekcja maja dostawac statusy evidence, nie tylko liczby.
- System ma deklarowac brak danych i powod: naturalny/rynkowy, insufficient sample, runtime source unavailable, config disabled, carried-forward.

## 15. Ostateczna zasada implementacyjna

Ten plan ma poprawic rzetelnosc systemu, nie kosmetyczny coverage.

Jesli implementacja zwieksza procenty obecnosci pol, ale traci informacje o tym, skad wartosc pochodzi, jaki ma sample count, czy byla carried, albo dlaczego byla missing, to jest regresja i nalezy ja odrzucic.

Poprawny system po tych zmianach ma byc bardziej precyzyjny, rzetelny i transparentny:

```text
gdy nie ma danych, mowi ze ich nie ma;
gdy dane sa slabe, mowi ze sa slabe;
gdy wartosc jest przeniesiona z ciszy, mowi ze jest przeniesiona;
gdy metryka jest policzona, mowi z jakiej proby i wedlug jakiej polityki;
gdy policy uzywa wartosci, loguje dlaczego wolno bylo jej uzyc.
```
