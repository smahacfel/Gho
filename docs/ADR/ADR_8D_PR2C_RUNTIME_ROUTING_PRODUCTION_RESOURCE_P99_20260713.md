# ADR-8D: PR2C — canonical routing, produkcyjny resource path i finite p99

Status: `IMPLEMENTED / FULL VALIDATION IN PROGRESS`

Typ: ADR-8D / third review amendment / DecisionLogger routing / durability /
resource metrology

Data: 2026-07-13

Repo: `smahacfel/Gho`

Branch: `agent/metric-contract-pr2c-durable-evidence-replay`

Base i merge-base: `fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9`

Plan normatywny:
`PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`

Raport dowodowy:
`reports/metric_contracts/pr2c_durable_evidence_replay_verification_v1.md`

Poziom ryzyka: `HIGH`. Amendment dotyka terminalnego routingu v33/v34/evidence,
bounded kolejki `DecisionLogger`, ciągłego zegara resource i frozen BURN
provenance. Nie zmienia aktywnej Gatekeeper policy, authority Profile A,
terminalnego verdictu, reason codes ani rollout mode `Legacy`.

## 1. Problem

Trzecie blocking review wskazało trzy kontrprzykłady integracyjne:

1. `GatekeeperAssessment::to_buy_log()` poprawnie tworzy surowy v33 bez file
   routing provenance, natomiast PR2C pair był budowany przed należącym do
   `DecisionLogger` plane expansion/hydration. Brak `decision_plane` albo
   `config_hash` usuwał pair przez fail-closed `.ok()`, podczas gdy v33 nadal
   mógł zostać zapisany.
2. Release harness wywoływał paired writer bezpośrednio. Pomijał bounded queue,
   poprzedzający command v33, jego filesystem I/O i scheduler delay, choć
   produkcyjny ciągły `Instant` obejmuje te przerwy.
3. Frozen histogram kończył finite bounds na `2_000 us`. Dla każdej próbki
   powyżej tej wartości percentile helper zwracał `max_us`, przez co wymaganie
   p99 `<= 5_000 us` stawało się w praktyce hard-max gate.

## 2. Decyzja: routing pozostaje własnością DecisionLogger

Wprowadzono trzy typed elementy:

```text
DecisionLoggerRoutingContextV1
RoutedGatekeeperDecisionV1
Pr2cRoutedDecisionContextV1
```

`DecisionLogger` wykonuje istniejący plane expansion i routing hydration raz,
przed konsumpcją terminalnego metric-contract snapshotu. Wynik zachowuje
prywatną kolekcję exact routed v33 rows. Z legacy-live row wyprowadzany jest
immutable PR2C context:

```text
record_identity = (run_id, join_key, decision_plane)
gatekeeper_config_hash
brain_config_hash
```

Hash i identity są parsowane fail-closed do typów domain. Pair builder nie
odczytuje już routingu z surowego `GatekeeperBuyLog`. Następnie do kolejki
wchodzi dokładnie ten sam `RoutedGatekeeperDecisionV1`, z którego pobrano
context, a po nim pair:

```text
assessment.to_buy_log()            # routing fields mogą być None
→ runtime observation enrichment   # m.in. canonical join_key
→ DecisionLogger::route_gatekeeper_buy_decision()
  ├─ exact routed legacy_live row → Pr2cRoutedDecisionContextV1 → pair
  └─ ten sam immutable routed row set → v33 command
→ WriteGatekeeperBuy
→ WriteMetricContractPair
```

Nie mutujemy surowego v33 do `legacy_live` przed expansion. Zachowuje to
istniejące generowanie `legacy_live` i `v25_shadow` oraz jeden owner file
routingu. Typed routing error zwiększa
`metric_contract_pair_build_failures_total`; snapshot nie jest konsumowany,
dopóki routed context nie przejdzie walidacji.

## 3. Graceful finalization jako testowalna granica

`DecisionLogger::shutdown()` używa `oneshot` acknowledgement. Logger odpowiada
dopiero po przetworzeniu wcześniejszych commandów, finalizacji paired writer,
`sync_data` partów, `sync_all` manifestu i directory sync. Dzięki temu test E2E
nie polluje przypadkowo nieukończonego manifestu i może sprawdzić durable stan
po tej samej kolejce co runtime.

## 4. Produkcyjny release resource path

Normatywny `metric_contract_build_and_serialize_us` nadal używa jednego
monotonicznego `Instant`, utworzonego przed pierwszym canonical producer call.
Release harness nie wywołuje już writera bezpośrednio. Dla każdej mierzonej
próbki wykonuje:

```text
first canonical producer call
→ full evidence/projection build i validation
→ rzeczywista druga policy evaluation
→ DecisionLogger canonical routing
→ terminal pair
→ bounded channel: routed v33 command
→ bounded channel: paired command
→ v33 expansion result write na właściwej plane path
→ paired writer timestamp/rotation binding
→ exact final evidence bytes
→ exact final v34 bytes
→ histogram sample z tego samego Instant
→ finalized manifest
```

W szczególności wall-clock sample obejmuje queue admission, scheduler delay i
I/O poprzedzającego v33 commandu. Kończy się po utworzeniu dokładnych finalnych
bajtów bieżącego paira, przed write tych bajtów. Harness odczytuje acceptance
histogram z finalized production manifestu i nie dopisuje syntetycznego
`enqueue_wait_us=0`.

## 5. Finite histogram codebook dla gate 5 ms

Frozen latency histogram codebook version 2 ma bounds:

```text
1, 2, 4, 8, 16, 32, 64, 128, 256, 512,
1_000, 2_000, 2_500, 3_000, 3_500, 4_000, 4_500, 5_000 us,
overflow
```

P99 w finite bucket zwraca jego upper bound. `max_us` pozostaje niezależną
obserwacją. Regresja `999 × 3_000 us + 1 × 20_000 us` wymaga:

```text
p99 upper bound = 3_000 us
max_us = 20_000 us
```

Audit nadal sprawdza exact bounds, checked sum bucketów, `sample_count`, jedną
próbkę na paired command oraz spójność highest populated bucket/overflow/max.

## 6. BURN_IN_CONTRACT_V3

Zmiana histogram codebooku jest zmianą frozen gate provenance. Ponieważ nie
rozpoczęto żadnego prospective runu, V2 jest jawnie superseded pre-run
artifactem, ale nie jest cicho reinterpretowany. Bieżący kontrakt:

```text
artifact: reports/metric_contracts/BURN_IN_CONTRACT_V3.json
burn_in_contract_version: 3
latency_histogram_codebook_version: 2
frozen_at: 2026-07-13T21:30:25Z
owner approval identity:
  github:smahacfel:authorized-pr2c-5ms-p99-codebook-amendment:2026-07-13
canonical SHA-256:
  fe363f6730ac8ce554b79f0044de90eba1d9583e4e701ccf84071c0d3e352e57
```

V1 był pre-run draftem limitu 1 ms. V2 zamroził autoryzowany limit 5 ms, lecz
nie miał finite bucketu rozstrzygającego ten p99 gate. Żaden row V1/V2 nie może
wejść do bundle V3.

## 7. Regresje acceptance

Dodane regresje obejmują:

- real `GatekeeperAssessment::to_buy_log()` z routing fields `None`;
- jedyne runtime-like uzupełnienie canonical `join_key`;
- real DecisionLogger routing i exact legacy-live context;
- real v33-before-pair bounded queue ordering;
- dokładnie jeden legacy v33, jeden v34 i jeden evidence row;
- exact three-way record identity equality;
- finalized manifest i single-run audit różny od `FAIL_SCHEMA_OR_REPLAY`;
- finite-p99/rare-overflow truth table;
- release harness przez production DecisionLogger path.

## 8. Zachowane kontrakty

Bez zmian pozostają:

- `GatekeeperBuyLog` v33 schema i historical parser;
- active Gatekeeper V2/V2.5/V3 thresholds, weights, phases, soft points,
  verdicts i reason codes;
- authority Profile A i rollout `Legacy`;
- V3 replay v1, selector score, IWIM, sender, Jito, execution i post-buy;
- jeden PR2B frozen producer snapshot oraz brak producer rerun;
- v34 exact field-set, semantic evidence hash i replay v2;
- brak PR3, Type-5 T1, DualCompute i V2 rollout.

## 9. Konsekwencje i decyzja

Pozytywne:

- v33, v34 i evidence mają jeden canonical routed identity przed konsumpcją
  snapshotu;
- release job mierzy rzeczywiste command ordering i bounded queue;
- p99 jest statystycznym gate'em, a nie ukrytym hard max;
- zmiana codebooku ma nową, machine-readable BURN wersję i hash.

Koszt:

- routing jest wykonywany synchronicznie przed pair construction, ale nie robi
  I/O ani await;
- graceful shutdown czeka na durable finalization;
- codebook V2/V3 artifact pozostają historycznie widoczne, lecz tylko V3 jest
  bieżącym prospective contractem.

Pełne markery readiness mogą zostać przywrócone dopiero po czystym commicie,
E2E provenance test, release harnessie oraz pełnej macierzy PR2A/PR2B/PR2C.
