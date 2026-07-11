# ADR-8D: Rewizja V1.1 planu korekty kontraktów interpretacyjnych metryk

Status: `ACCEPTED / PLAN_V1_1_SAVED / DOCUMENTATION_ONLY`

Typ: ADR-8D / implementation-plan revision

Data: 2026-07-11

Repo: `/root/Gho_dynamic_exit_v1`

Plan:
`PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`

Amendment po PR0 review:
`docs/ADR/ADR_8D_PR0_PROVENANCE_AND_REPRODUCIBILITY_CORRECTIONS_20260711.md`.
Niniejszy ADR zachowuje status planu w chwili zapisu; amendment normatywnie
domyka RFC 8785 hashing, effective config hash i record/event identity przed
PR1 oraz aktualizuje status PR0 do PASS.

Raport źródłowy:
`PLANS/AUDYT/RAPORT_AUDYT_KOREKTY_INTERPRETACJI_METRYK_20260710.md`

Poziom ryzyka tej zmiany: `LOW` — wyłącznie dokumentacja; przyszła implementacja
pozostaje `HIGH` dla SSOT, policy parity, replay i hot-path evidence logging.

Uwaga o szablonie: `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie.
Dokument zachowuje sekcyjny format ADR-8D stosowany w repo.

## 1. Kontekst

Pierwsza wersja planu poprawnie chroniła `MaterializedFeatureSet`, replay,
addytywną kompatybilność i rozdzielenie evidence od policy. Review wykazało
jednak blokujące luki:

- jeden globalny `Legacy/DualCompute/V2` nie określał per-contract authority;
- jeden PR2 obejmował zbyt wiele crates i niezależnych ryzyk;
- brakowało formalnego reconciliation z elementami już istniejącymi w kodzie;
- dev-buy łączył TxIntel first-observed i GatekeeperBuffer primary surfaces;
- flip V2 nie definiował jednoznacznego buy anchor i cumulative state;
- statusy jakości nie miały jednego canonical envelope;
- manipulation bool defaults nie rozwiązywały raw numeric missing-versus-zero;
- burn-in był zależny od jednego runa i dopuszczał błędne utożsamienie
  empirycznego zero drift z formalną równoważnością;
- plan nie miał budżetu rozmiaru JSONL, queue/backpressure i rotacji.

Dodatkowy code reconnaissance potwierdził:

- top3 helper już istnieje i nie powinien być implementowany ponownie;
- aktywny Phase 5 konsumuje MFS TxIntel first-observed dev buy;
- GatekeeperBuffer primary creator buy jest osobną compat powierzchnią;
- FTDI value używa unique buyers, ale legacy degraded gate używa całkowitej
  liczby BUY;
- decision schema ma wersję 33;
- istniejące decision rows są już duże, co uzasadnia sidecar zamiast osadzania
  wszystkich typed evidence w głównym rekordzie.

## 2. Decyzja

Zastąpiono treść planu V1 wersją V1.1 pod tą samą ścieżką, zgodnie z decyzją
właściciela planu. Nie utworzono drugiego pliku planu.

Historyczny status wykonawczy w chwili zapisu, zastąpiony po PR0 przez amendment
wskazany wyżej:

```text
PLAN_V1_1_ACCEPTED
PR0_BASELINE_RECONCILIATION_ALLOWED
RUNTIME_IMPLEMENTATION_BLOCKED_UNTIL_PR0_PASS
```

V1.1 zachowuje trzy milestones, ale rozdziela pracę na sześć realnych PR-ów:

```text
PR0 baseline reconciliation, bez kodu runtime
PR1 registry/profile/status foundation
PR2A active/parity-sensitive producers
PR2B evidence-only producers
PR2C v34/sidecar/comparator/replay/audit
PR3 equivalence-only cutover
```

## 3. Authority i rollout

Globalny rollout mode pozostaje ceiling:

```text
Legacy → DualCompute → V2
```

Dodano wersjonowany, hashowany Profile A. `V2` może aktywować tylko entries
oznaczone `EquivalentCutover`; nie oznacza globalnej promocji wszystkich nowych
metryk.

Profile A pozostawia jako non-authoritative:

- dev-primary,
- corrected FTDI unique-buyer actionability,
- same-ms `<50 ms`,
- flip V2,
- FSC v2,
- reserve velocity,
- recent buy/sell,
- coordination-risk.

PR3 może objąć wyłącznie formalnie równoważne typed representations istniejących
sygnałów. Empiryczny brak driftu nie zastępuje dowodu równoważności.

## 4. Kontrakty semantyczne

Przyjęto:

- surface-qualified dev-buy registry rozdzielający TxIntel, GatekeeperBuffer,
  MFS first-observed, MFS primary V1 i effective policy read;
- FTDI value i FTDI actionability jako osobne kontrakty;
- first-eligible-buy anchor dla flip V2, brak re-anchor, cumulative buys/sells,
  first qualifying sell freeze, stable identity/order i fail-closed gaps;
- canonical `MetricEvidenceEnvelopeV1` z availability, measurement quality,
  authority class, actionability i typed reasons;
- jawne adaptery z istniejących status enums zamiast ich destrukcyjnego usuwania;
- presence-aware `ManipulationNumericEvidenceV2`, aby missing raw field nie
  udawał realnego zera;
- legacy FSC i FSC v2 jako oddzielne statusy, bez promocji FSC v2.

## 5. Logging i replay

Decision schema v34 ma być compact summary. Pełne typed evidence trafia do:

```text
metric_contract_evidence_v1.jsonl
```

Decision i sidecar łączy:

```text
(run_id, join_key, decision_plane)
```

oraz evidence record ID/hash. Missing pair, writer failure, ENOSPC, malformed
part, hash mismatch albo manifest mismatch dyskwalifikuje run.

Przyjęto budżety comparator/serialization/enqueue p99, queue high-water, zero
drops/failures/orphans, limity bytes per record i maksymalny 25% wzrost GB/hour.

Replay v1 pozostaje frozen. Replay v2 weryfikuje profile/config/evidence hashes i
nie korzysta z hidden runtime state.

## 6. Feasibility i validation discipline

Burn-in jest bundle co najmniej trzech immutable, niepokrywających się runów,
każdy o długości minimum 1 h, obejmujących co najmniej dwa UTC 4-hour buckets.
Każdy run musi osobno przejść full replay/schema/hash/resource gates przed
agregacją.

Początkowa hipoteza minimów pozostaje:

```text
8 h
700 decisions
100 dev-known
100 clean flip-v2 evaluable
30 dev legacy/v2 divergences
```

Historyczny feasibility audit może je kontrolowanie podnieść, obniżyć,
potwierdzić lub rozłożyć na per-run/aggregate — wyłącznie przed prospective
validation.

Po zatwierdzeniu powstaje hashowany `BURN_IN_CONTRACT_V1` z `frozen_at`.
Feasibility rows nie liczą się do validation. Zmiana gate po zobaczeniu
niekorzystnego validation result unieważnia cały bundle i wymaga nowej wersji,
nowego `frozen_at` oraz całkowicie nowych runów.

## 7. Granice

Ta rewizja nie daje zgody na:

- runtime implementation przed `BASELINE_RECONCILIATION_PASS`;
- zmianę BUY/REJECT/TIMEOUT, progów, wag, reason codes lub selector score;
- policy promotion semantycznie nowych metryk;
- live-state reads podczas policy;
- MFS bypass lub GatekeeperBuffer jako drugi SSOT;
- destructive schema/config migration;
- zmianę IWIM, post-buy, sendera lub live execution;
- post-hoc dostosowanie minimów do niekorzystnego wyniku.

## 8. Pliki tej zmiany

Zmieniono lub utworzono wyłącznie:

- `PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`
- `docs/ADR/ADR_8D_PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`
- `docs/ADR/ADR_8D_PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_1_20260711.md`

Nie zmieniono Rust, Python, TOML, testów ani runtime artifacts.

## 9. Weryfikacja zapisu

Wymagane:

- plan ma status V1.1 i pozostaje pod wcześniejszą ścieżką;
- zawiera PR0/PR1/PR2A/PR2B/PR2C/PR3;
- zawiera authority profile, dev surface matrix i flip state machine;
- zawiera canonical status envelope i manipulation presence contract;
- zawiera compact v34/sidecar/resource budgets;
- zawiera multi-run bundle i anty-post-hoc freeze rule;
- zawiera wszystkie 10 metryk;
- brak trailing whitespace i niedomkniętych code fences;
- `git status` potwierdza brak ingerencji w niezwiązane zmiany użytkownika.

Testów runtime nie uruchamia się dla documentation-only rewizji.

```yaml
delegation_trace:
  task_classification: localized persistence of accepted cross-cutting plan revision
  routing_performed: true
  primary_specialist: Ghost Runtime Coordinator
  supporting_specialists_considered:
    - SSOT Feature Materialization Guardian
    - Gatekeeper Policy Auditor
    - Decision Logging Replay Analyst
    - Config Rollout Safety Reviewer
    - Seer Ingest Event Integrity Specialist
  specialist_docs_loaded: []
  specialist_docs_not_loaded:
    - name: specialist documents
      reason: this task persists the already reviewed and accepted V1.1 plan without new architecture decisions
  skills_used:
    - ghost-execution
  fast_path_used: true
  contracts_checked:
    - fidelity to accepted V1.1
    - same-path replacement requested by user
    - mandatory ADR creation
    - documentation-only scope
  unresolved_routing_uncertainty: []
```
