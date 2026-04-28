# ADR-0102: Authoritative funding lane unlock must be delivered as four additive PRs

**Date:** 2026-04-17
**Status:** Accepted
**Author:** Ghost Father

## Context

Repozytorium posiada już lokalny i runtime'owy szkielet dla `FSC` (`funding_source_concentration`), ale aktualna ścieżka funding transferów pozostaje semantycznie częściowa:

- obecny `grpc_global_stream` jest filtrowany do Pump.fun / PumpSwap traffic,
- Seer emituje funding transfers z `full_chain_coverage=false`,
- launcher startuje fail-closed i nie powinien udawać authoritative funding readiness bez rzeczywistego full-feed lane,
- `ADR-0092`, `ADR-0096`, `ADR-0097` i `ADR-0101` zamykają granice: canonical feature path przez `MaterializedFeatureSet`, brak RPC hot-path, brak pool-local heurystyk i brak kar za degraded state.

Problem nie polega więc na "braku jednej flagi", tylko na braku bezpiecznej kolejności wdrożenia. Próba rozwiązania tego jednym dużym PR-em albo prostym flippem `full_chain_coverage` / `authoritative_funding_stream_available` niosłaby wysokie ryzyko naruszenia SSOT, replay determinism i fail-closed semantics.

## Decision

Odblokowanie authoritative `FSC` ma zostać wykonane jako **cztery additive PR-y** z rosnącym blast-radiusem i jawnymi rollback boundaries.

### Przyjęta kolejność

1. **PR-1 — Contract freeze + additive funding provenance contract**
   - zamrozić granice semantyczne,
   - doprecyzować transport funding provenance,
   - utrzymać pełną backward compatibility,
   - bez zmiany zachowania runtime.

2. **PR-2 — Seer authoritative funding lane (disabled by default)**
   - dodać osobny full-feed funding lane,
   - nie zmieniać znaczenia istniejącego `grpc_global_stream`,
   - nie wpływać na stary trade/pool detection przy domyślnej konfiguracji.

3. **PR-3 — Launcher/runtime readiness wiring**
   - zastąpić startupowy hardcode availability sygnałem opartym o rzeczywisty authoritative lane,
   - utrzymać `FundingSourceIndex` jako jedyny stateful source dla `FSC`,
   - zachować fail-closed semantics i canonical materialization path.

4. **PR-4 — Observability + bake package + rollout guardrails**
   - dodać pełną obserwowalność lane health i FSC readiness,
   - przygotować replay/paper-burnin bake,
   - nadal nie aktywować policy penalties `FSC`.

### Jawne ograniczenia decyzji

- `grpc_global_stream` pozostaje filtered trade lane i nie może zostać semantycznie „awansowany” do full coverage przez rename albo flip booleana.
- `full_chain_coverage=true` wolno ustawić wyłącznie dla eventów z dedykowanego authoritative funding lane.
- Policy activation `FSC` pozostaje poza zakresem tej decyzji i wymaga osobnego follow-upu po bake.
- Nie wykonujemy szerokiego globalnego refaktoru `source_label` w pierwszym kroku; preferowany jest additive provenance contract dla funding transportu.

## Architectural Impact

Decyzja utrwala następujący podział odpowiedzialności:

1. **Seer/data-plane**
   - rozdziela filtered trade lane od authoritative funding lane,
   - wystawia additive provenance contract dla funding transfers,
   - utrzymuje backward-compatible IPC.

2. **Launcher/runtime**
   - nie zgaduje funding readiness,
   - konsumuje authoritative signal fail-closed,
   - nadal materializuje `FSC` wyłącznie przez `FundingSourceIndex` i `MaterializedFeatureSet`.

3. **Policy**
   - pozostaje poza zakresem unlocku,
   - nie dostaje żadnych skrótowych obejść ani bezpośrednich odczytów z indexu.

4. **Rollout**
   - każdy PR jest merge'owalny niezależnie,
   - domyślny config pozostaje bezpieczny,
   - rollback po każdym etapie nie wymaga naruszania SSOT surfaces.

## Risk Assessment

**Rate:** Medium

- **Low** ryzyko dla PR-1, jeśli pozostanie czysto additive i kontraktowy.
- **Medium** ryzyko dla PR-2, bo dotyka Seer subscribe/data-plane, ale jest ograniczone przez domyślne wyłączenie nowego lane'u.
- **Medium** ryzyko dla PR-3, bo dotyka runtime readiness, ale pozostaje fail-closed i bez policy activation.
- **Low/Medium** ryzyko dla PR-4, bo dotyczy głównie observability i rollout scaffolding.
- **Critical** byłoby rozwiązanie odrzucone: prosty flip `full_chain_coverage=true` lub startupowego availability bez rzeczywistego authoritative lane.

## Consequences

Co staje się łatwiejsze:

- wdrożenie jest odwracalne po każdym etapie,
- blast radius jest kontrolowany,
- SSOT i replay boundaries pozostają nienaruszone,
- operatorzy dostają jasny bake path zanim `FSC` stanie się policy-effective.

Co staje się trudniejsze:

- potrzeba większej dyscypliny w kolejności wdrożenia,
- część zmian dokumentacyjnych i testowych musi wejść przed realnym „feature unlockiem”,
- pełna aktywacja `FSC` zajmuje więcej niż jeden PR.

## Alternatives Considered

### 1. Jeden duży PR: contract + lane + runtime + policy

Odrzucono.

Łączyłby zbyt wiele warstw naraz i utrudniłby izolację regresji w Seerze, runtime i policy.

### 2. Ustawić `full_chain_coverage=true` dla obecnego `grpc_global_stream`

Odrzucono.

To narusza prawdomówność kontraktu, bo obecny stream nie jest full-chain funding feedem.

### 3. Ustawić `authoritative_funding_stream_available=true` na starcie bez nowego lane'u

Odrzucono.

To łamie fail-closed semantics i pozwala runtime udawać readiness bez dowodu coverage.

### 4. Zacząć od globalnego refaktoru wszystkich `source_label` na enum

Odrzucono na pierwszy krok.

To zwiększa blast radius bez bezpośredniego odblokowania authoritative funding lane. Jest to ewentualny późniejszy cleanup, nie prerequisite.

## Validation Steps

1. PR-1:
   - potwierdzić additive serde compatibility,
   - potwierdzić brak behavior drift,
   - potwierdzić, że obecny filtered lane nadal nie może produkować `full_chain_coverage=true`.

2. PR-2:
   - potwierdzić, że authoritative funding lane jest osobny i disabled by default,
   - potwierdzić brak regressji w trade/pool detection,
   - potwierdzić lane separation testami.

3. PR-3:
   - potwierdzić, że runtime readiness nie promuje availability przed dowodem coverage,
   - potwierdzić, że `FundingSourceIndex` pozostaje jedynym stateful source dla `FSC`,
   - potwierdzić brak decision drift przy authoritative lane disabled.

4. PR-4:
   - potwierdzić istnienie pełnej obserwowalności lane health i FSC readiness,
   - potwierdzić replay/paper-burnin bake checklist,
   - potwierdzić, że policy activation `FSC` nadal nie następuje w tym etapie.

5. Po PR-1…PR-4:
   - dopiero po bake rozważyć osobny follow-up dla policy `FSC` zgodny z `ADR-0097`.
