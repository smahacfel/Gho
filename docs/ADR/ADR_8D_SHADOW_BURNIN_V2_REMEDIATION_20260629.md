# ADR-8D: Shadow Burnin Simulation V2 Remediation Contract

Data: 2026-06-29

Status:

```text
ACCEPTED_FOR_PR1_CONTRACT
```

## D1. Problem

P0 Shadow Burnin Fidelity Audit zwrócił:

```text
SHADOW_REPLAY_LIFECYCLE_MISMATCH
```

Obecny Shadow V1 nie daje jednej spójnej prawdy pozycji. `shadow_lifecycle` i `shadow_exit_replay_v1` nie mogą być traktowane jako unified terminal truth. Entry price nie jest udowodniony jako executable live fill, a exit price jest offline path/mark evidence, nie live sell fill.

## D2. Decision

Definiujemy Shadow Burnin Simulation V2 jako nowy, dodatni kontrakt:

- side-by-side z V1,
- domyślnie disabled,
- schema-first,
- event-sourced,
- z jednym canonical position event stream,
- z rozdzieleniem `MARK_PRICE_REPLAY`, `EXECUTABLE_FILL_SIM`, `LIVE_CONFIRMED`.

PR1 wprowadza tylko kontrakty, typy i artefakty schema/acceptance. PR1 nie aktywuje runtime writerów ani nie zmienia semantyki runtime.

## D3. Evidence

Root evidence:

- `PLANS/AUDYT/RAPORT_SHADOW_BURNIN_FIDELITY_AUDIT_20260629.md`
- `docs/ADR/ADR_8D_SHADOW_BURNIN_FIDELITY_AUDIT_20260629.md`
- `PLANS/AUDYT/RAPORT_SHADOW_FIDELITY_DOWNGRADE_IMPACT_20260629.md`
- `docs/ADR/ADR_8D_SHADOW_FIDELITY_DOWNGRADE_IMPACT_20260629.md`
- `reports/selector/shadow_fidelity_claim_evidence_matrix.csv`

New PR1 contract evidence:

- `docs/SPEC/SHADOW_BURNIN_V2_SIMULATION_CONTRACT_20260629.md`
- `reports/selector/shadow_v2_required_schema_manifest.csv`
- `reports/selector/shadow_v2_acceptance_gates.csv`
- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`

## D4. Root Cause

Shadow V1 miesza offline mark/path evidence z lifecycle evidence i nie ma jednego canonical terminal truth. Brakuje też wystarczającego kontraktu clock-domain, event ordering, pool-state provenance, fill model, failure/no-fill model i calibration data dla live-equivalence.

## D5. Corrective Action

PR1 ustanawia:

- common V2 envelope,
- `simulation_level`,
- `measurement_grade`,
- `event_order_key`,
- clock-domain contract,
- temporal class contract,
- canonical record set,
- acceptance gates,
- legacy downgrade matrix,
- PR12 fidelity-only boundary,
- PR14 live-confirmed calibration requirement.

## D6. Rejected Alternatives

Odrzucono:

- naprawianie V1 in-place,
- traktowanie `shadow_exit_replay_v1` jako terminal truth,
- traktowanie `shadow_lifecycle` jako terminal truth,
- domyślne zero latency / zero slippage / zero own impact,
- promowanie ShadowLedger do canonical live truth,
- uznanie static fill assumptions za live-equivalence.

## D7. Consequences

Do czasu przejścia bramek V2:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
R52_approval=false
strategy_research_unblocked=false
```

Stare raporty mogą być używane tylko jako downgraded offline mark/path diagnostics. Nie mogą być cytowane jako proof of live PnL, executable fills, live slippage behavior ani real landing outcome.

Bez PR14 live-confirmed calibration dataset maksymalny verdict to:

```text
SHADOW_V2_RESEARCH_GRADE_ONLY
```

## D8. Verification

PR1 verification:

- Rust contract serialization tests,
- schema manifest presence,
- acceptance gates manifest presence,
- downgrade matrix presence,
- risk register presence,
- `git diff --check`,
- forbidden staged file guard,
- no runtime run,
- no R51 touch.

Runtime boundary:

```text
NO_RUNTIME_SEMANTICS_CHANGED
```
