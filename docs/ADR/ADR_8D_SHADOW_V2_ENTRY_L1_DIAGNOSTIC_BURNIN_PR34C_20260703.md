# ADR-8D: Shadow V2 Entry L1 Diagnostic Burnin PR34-C

Data: 2026-07-03

Status:

```text
ACCEPTED_AS_DIAGNOSTIC_L1_ENTRY_EVIDENCE
```

## D1. Problem

Po PR33 + PR34-A/B/C trzeba bylo sprawdzic, czy runtime potrafi w realnym shadow flow przeniesc upstream `ENTRY_BEFORE` boundary payload do PostBuyRuntime i zasilic nim ShadowV2FillEngine dla entry fill.

Poprzednio Shadow V2 mial L0 harness oraz canonical writer, ale brakowalo dowodu, ze L1 deterministic execution simulation rzeczywiscie dostaje pre-entry pool-state boundary w runtime.

## D2. Decyzja

Wykonano jeden operator-approved validation/fidelity burnin:

```text
run_id = shadow-burnin-v2-entry-l1-diagnostic-pr34c-r1
scope = reports/selector/shadow-v2-entry-l1-diagnostic-pr34c-r1
main_head = 57c6e313e2b583ea09bde98d0eff5367fd048f88
shutdown = SIGINT_CLEAN
```

Finalny verdict:

```text
PR34_ENTRY_L1_DIAGNOSTIC_BURNIN_PASS
```

Decyzja jest waska: zaakceptowano dowod diagnostic L1 entry fill wiring. Nie nadano research-grade, live-equivalence ani runtime approval.

## D3. Evidence

Runtime/shutdown:

- pre-run manifest: PASS
- pre-run strict audit: PASS
- validation burnin plan audit: PASS / `FIDELITY_ONLY`
- legacy downgrade audit: PASS
- runtime post-run manifest: PASS
- post-run strict audit: PASS
- `PostBuyRuntime: Shadow V2 post-run manifest generated and strict-verified`
- `PostBuyRuntime shut down successfully`
- `All components shut down successfully`
- `Ghost Launcher shutdown complete`
- SIGTERM/SIGKILL: false

Canonical evidence:

| Metric | Value |
|---|---:|
| accepted shadow handoffs | 81 |
| entry attempts | 81 |
| pool state samples | 163 |
| entry fills | 81 |
| entry boundary payloads | 81 |
| entry fill FILLED | 81 |
| entry fill BLOCKED_BY_DATA | 0 |
| execution_simulation_ready=true | 81 |
| execution_label_grade=DIAGNOSTIC_SIM | 81 |
| execution_label_grade=RESEARCH_CANDIDATE | 0 |
| execution_label_grade=LIVE_CONFIRMED | 0 |

Handoff validation:

| Blocker | Count |
|---|---:|
| `ENTRY_BOUNDARY_BASE_MINT_MISMATCH` | 0 |
| `ENTRY_BOUNDARY_POOL_ID_MISMATCH` | 0 |
| `ENTRY_BOUNDARY_HANDOFF_VALIDATION_FAILED` | 0 |
| `ENTRY_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS` | 0 |
| `ENTRY_FILL_POOL_STATE_AFTER_FILL_BOUNDARY` | 0 |
| `ENTRY_FILL_POOL_STATE_NOT_STRICTLY_BEFORE_FILL_BOUNDARY` | 0 |
| unknown/untyped blockers | 0 |

## D4. Limitations

1. `research_provenance_ready=true` wystapilo 0 razy.
2. `POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME` dotyczy 81 entry fills i wystepuje 162 razy jako label occurrence.
3. Chain-order ma explicit `UNKNOWN` components, wiec temporal audit pozostaje `BLOCKED_TEMPORAL_AMBIGUITY_REMAINS`.
4. `BLOCKED_ORDERING_AMBIGUITY` jest obecny jako limitation/provenance label, nie jako `fill_status=BLOCKED_BY_DATA` dla entry fill.
5. Brak live-confirmed fills, landing/failure telemetry, realized slippage i quote/fill divergence.
6. Ten burnin nie obejmuje executable exit fill ani complete executable roundtrip.

## D5. Rejected alternatives

Odrzucono:

- traktowanie diagnostic `FILLED` jako live fill;
- traktowanie braku `account_data_hash` jako research-grade provenance;
- traktowanie explicit UNKNOWN chain-order jako PASS research-grade;
- nadawanie runtime approval, shadow_close_only approval albo active close approval;
- commitowanie raw JSONL, logow, runtime scope albo lokalnych TOML.

## D6. Runtime boundary

Nie zmieniono i nie wolno wywodzic z tego ADR zmiany:

```text
BUY/REJECT = unchanged
Gatekeeper policy = unchanged
selector runtime = unchanged
TX/Jito/live path = unchanged
shadow_close_only = disabled / not approved
active close = disabled / not approved
R51 = untouched
```

## D7. Consequences

1. PR33 + PR34-A/B/C maja runtime evidence, ze `ENTRY_BEFORE` boundary payload dociera do L1 engine.
2. Entry fill diagnostic simulation jest gotowa do dalszego validation, ale tylko jako `DIAGNOSTIC_SIM`.
3. Research-grade pozostaje zablokowany przez provenance/ordering limitations.
4. Kolejny etap powinien osobno adresowac provenance/ordering albo przejsc do analogicznego exit-side L1 work, ale nie wolno z tego burnina nadac live-equivalence.

## D8. Final flags

```text
runtime_approval = false
shadow_close_only_approval = false
active_close_approval = false
research_grade = NOT_GRANTED
live_equivalence = NOT_GRANTED
strategy_research_unblocked = false
```
