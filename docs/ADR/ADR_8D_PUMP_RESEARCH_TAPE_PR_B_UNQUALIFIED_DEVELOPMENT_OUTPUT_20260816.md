# ADR-8D: Pump Research Evidence Tape V1.1 — PR-B development materialization bez qualification promotion

**Data:** 2026-08-16

**Status:** PR-B STARTED / DEVELOPMENT-ONLY / EXPORT AND STRATEGY NO-GO

**Task:** `PUMP_RESEARCH_TAPE_PR_B_UNQUALIFIED_DEVELOPMENT_OUTPUT`

## D0. Stan

PR-B został rozpoczęty po utworzeniu historycznego raw runu. Kod posiada
offline materializer i CLI `certify` / `export-window`; nie zmienia to granic
PR-A ani active runtime.

Powstały dwa materialization artifacts dla source runu
`pump-research-1786810567606-3429034`:

```text
.exact-prb-20260816-1.partial = przerwany, nieopublikowany lifecycle evidence
exact-prb-20260816-2          = opublikowany development output
```

`exact-prb-20260816-2/manifest.json` deklaruje:

```text
schema_version       = 1
source_run_id        = pump-research-1786810567606-3429034
qualification_status = Unqualified
```

## D1. Decyzja o klasyfikacji

Output pozostaje:

```text
UNQUALIFIED / development-only / no export / no strategy
```

Materializer może służyć do rozwoju parsera, inventory, canonicality,
trajectory certifiera oraz regresji na zachowanym materiale. Nie może jednak
zamienić historycznego raw runu w dowód source completeness, build provenance
ani provider qualification.

`export-window` wymaga independently qualified Ready exact tape; status
`Unqualified` ma pozostać fail-closed. Nie wolno obniżać tej bramki dla RIFT,
innej strategii, dashboardu ani ręcznego eksperymentu.

## D2. Zakres PR-B, który pozostaje research-only

PR-B rozwija complete transaction-local mutation inventory, Create/CreateV2,
minimalny Pump Global dependency, slot canonicality, trajectory certification,
participant token-account evidence oraz offline qualification findings. Jest
addytywny względem runtime-compatible parser outputs i nie modyfikuje:

```text
canonical permit / AccountStateCore authority / Gatekeeper / MFS
execution / sender / live quote authority / SeerConfig active runtime
```

W szczególności historyczny raw run nie uprawnia do deklaracji
`PUMP_RESEARCH_TAPE_V1_READY`. Do tego potrzebny jest replacement raw run z
poprawioną provenance/auth policy oraz independent source-completeness audit.

## D3. Zachowanie i rollback

Nie usuwamy `.exact-prb-20260816-1.partial` ani publikowanego exact outputu.
Nie nadpisujemy ich manifestu ani nie dopisujemy do nich qualification results.
Rollback polega na niewykonywaniu `certify`/`export-window` wobec tych
artefaktów poza jawnie oznaczonym developmentem offline.
