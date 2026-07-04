# ADR-8D: Shadow V2 L2-D Density Horizon Retention Contract 20260704

## Status

Accepted implementation stage.

## Decision

Dodajemy L2-D offline audit contract dla Shadow V2 path density, horizon i
retention:

```text
final_verdict=BLOCKED_RETENTION_CONTRACT_INSUFFICIENT
contract_implementation=L2_D_DENSITY_HORIZON_CONTRACT_IMPLEMENTED
```

Decyzja architektoniczna: pierwszy L2 baseline ocenia tylko jawnie declared
horizons. Long horizons 300s/500s pozostaja undeclared i nie moga blokowac
baseline ani wspierac pozytywnego research claim.

## Context

Po L2-C mamy rozdzielony transaction source proof i account-state source proof.
Nadal brakowalo formalnego density/horizon contract. Stary offline audit
przepuszczal dowolny `EVALUABLE_*` albo `SPARSE_APPROX_ONLY` row, zamiast
sprawdzac komplet declared horizons, retention i replay coverage.

L2-D zamienia ten audit w fail-closed metrology gate.

## Implemented Contract

Declared horizons dla pierwszego baseline:

```text
2000
3000
10000
30000
120000
```

Undeclared long horizons:

```text
300000
500000
```

Long horizons sa klasyfikowane jako:

```text
NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
```

Retention contract:

```text
required_replay_horizon_ms = max(declared_supported_horizons_ms) + retention_margin_ms
```

Domyslnie:

```text
retention_margin_ms=1000
required_replay_horizon_ms=121000
```

## Code-Level Changes

W `scripts/shadow_v2_path_density_horizon_audit.py` dodano:

- CLI for declared horizons, undeclared horizons i retention margin;
- per-horizon L2 baseline blocker classification;
- per-horizon positive research claim guard;
- missing declared horizon rows;
- strict handling of `SPARSE_APPROX_ONLY`;
- strict retention/replay coverage gate;
- explicit false approval flags in audit output.

W `tests/test_shadow_v2_path_density_horizon_audit.py` dodano fixture tests dla
pass, missing declared horizon, sparse declared horizon i retention shortfall.

## Rejected Alternatives

### Treat 300s/500s as required baseline horizons

Rejected. Plan L2 deklaruje 300s/500s jako undeclared dla pierwszego baseline.
Wymaganie ich teraz powodowaloby false blocker i mieszaloby baseline z long
horizon research mode.

### Promote `SPARSE_APPROX_ONLY` to PASS

Rejected. Sparse approximation moze byc informacyjna, ale nie jest L2 baseline
density proof.

### Infer retention from row presence

Rejected. Row presence nie dowodzi, ze replay horizon pokrywa max declared
horizon plus margin.

### Modify runtime sampler in L2-D

Rejected. L2-D jest metrology contract + audit implementation. Runtime sampler
zmieniamy tylko wtedy, gdy osobny stage udowodni taka potrzebe.

## Consequences

1. Pierwszy L2 baseline nie failuje przez undeclared 300s/500s.
2. 300s/500s nie moga wspierac pozytywnych claims bez osobnej deklaracji.
3. Declared horizons musza miec complete coverage.
4. Retention gap blokuje L2 nawet wtedy, gdy pojedyncze rows sa evaluable.
5. Aktualny PR18C scope pozostaje zablokowany przez retention insufficiency.

## Compatibility

Nie zmieniono runtime evidence JSON schema. Nie zmieniono Rust runtime. Zmieniono
offline audit output schema przez dodanie L2-D metrics.

## Verification

```bash
python3 -m py_compile scripts/shadow_v2_path_density_horizon_audit.py tests/test_shadow_v2_path_density_horizon_audit.py
python3 tests/test_shadow_v2_path_density_horizon_audit.py
python3 scripts/shadow_v2_path_density_horizon_audit.py --scope-root reports/selector/shadow-v2-fidelity-validation-pr18c-45m-r1 --pretty
```

## Final Decision

```text
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
next_stage=L2-E_GATEKEEPER_COVERAGE_DENOMINATOR_STARVATION_AUDIT
```
