# ADR-8D: Shadow V2 L2-E Gatekeeper Coverage Denominator Audit 20260704

## Status

Accepted implementation stage.

## Decision

Dodajemy L2-E offline audit contract dla Gatekeeper coverage, candidate
denominator i threshold starvation:

```text
final_verdict=BLOCKED_CANDIDATE_UNIVERSE_DENOMINATOR_UNKNOWN
contract_implementation=L2_E_GATEKEEPER_COVERAGE_DENOMINATOR_AUDIT_IMPLEMENTED
```

Decyzja architektoniczna: L2-E jest metrology-only gate. Moze wykryc unknown
denominator, unknown/generic reject reasons albo threshold starvation, ale nie
moze luzowac Gatekeeper thresholds ani modyfikowac runtime decision path.

## Context

Po L2-D mamy formalny density/horizon/retention contract, ale L2 offline
research-grade nadal wymaga znanego denominatora. Bez tego research validation
moze miec poprawne pojedyncze rekordy, ale nadal nie wiadomo, z jakiego
candidate universe pochodzi probka ani czy Gatekeeper/observation thresholds
nie odcinaja jej przed checkpointami.

Stary `audit_gatekeeper_decision_vs_r2.py` analizuje Gatekeeper vs R2 outcomes,
ale L2-E potrzebuje wezszej bramki: denominator known, typed reject reasons,
checkpoint reach i starvation status. Dlatego dodajemy osobny script zamiast
rozszerzac R2 audit.

## Implemented Contract

Candidate denominator:

```text
candidate_universe_v1.jsonl
```

jest event-level denominator. Gatekeeper decision logs sa tylko contextem i nie
moga tworzyc denominator rows.

Denominator jest PASS-eligible tylko gdy:

```text
candidate_universe exists
candidate_universe rows > 0
eligible_denominator_count > 0
decision_logs_created_denominator_rows == 0
candidate_ids_from_decision_only == 0
```

Reject reason taxonomy:

```text
unknown_reason_count == 0
```

jest wymagane do pozytywnego L2-E verdict. Generic non-BUY buckets blokuja L2-E
verdict:

```text
BLOCKED_UNKNOWN_REJECT_REASON_BUCKETS
```

Threshold starvation jest blokujace, gdy znany denominator nie dociera do
Gatekeeper decision evidence albo gdy wszystkie Gatekeeper decisions sa
typed reject/timeout i `gatekeeper_buy_count == 0`.

## Code-Level Changes

W `scripts/shadow_v2_gatekeeper_coverage_denominator_audit.py` dodano:

- offline CLI dla candidate universe, candidate manifest, decision JSONL,
  decision root i summary CSV;
- JSON report output;
- optional `metric,value,notes` CSV output;
- fail-closed denominator validation;
- Gatekeeper decision join coverage;
- typed reject reason distribution;
- unknown/generic reason blocker;
- threshold starvation blocker;
- approval flags forced false.

W `tests/test_shadow_v2_gatekeeper_coverage_denominator_audit.py` dodano testy
dla:

- missing candidate universe;
- known denominator + typed reasons;
- generic reject reason blocker;
- typed reject-only starvation;
- manifest invariant failure when decision logs create denominator rows.

## Rejected Alternatives

### Reuse `audit_gatekeeper_decision_vs_r2.py` as L2-E gate

Rejected. R2 audit jest szerszy i outcome-oriented. L2-E jest denominator /
coverage / starvation gate i musi dzialac przed finalnym research validation
runem.

### Let Gatekeeper decision logs create denominator rows

Rejected. Decision logs moga byc contextem, ale denominator musi pochodzic z
event-level candidate universe. Inaczej sample bylby selection-biased.

### Treat zero BUY as automatic policy fix request inside L2-E

Rejected. L2-E moze wykryc starvation i zwrocic
`BLOCKED_GATEKEEPER_THRESHOLD_STARVATION`. Threshold tuning albo policy review
musza byc osobnym stage.

### Infer research candidate roundtrips from entry/exit counts separately

Rejected. Entry and exit counts can be positive for different positions.
`research_candidate_roundtrip_count` is accepted only as explicit metric.

## Consequences

1. L2-E ma osobny, powtarzalny offline audit gate.
2. Brak tracked candidate universe blokuje L2-E fail-closed.
3. Generic reject buckets sa widocznym blockerem, nie ukrytym warningiem.
4. Starvation jest klasyfikowana bez zmiany Gatekeeper policy.
5. PR41 diagnostic roundtrips nie sa traktowane jako denominator proof.

## Compatibility

Nie zmieniono runtime evidence JSON schema. Nie zmieniono Rust runtime. Nie
zmieniono Gatekeeper, BUY/REJECT, selector runtime, TX/Jito/live path,
`shadow_close_only` ani active close. Zmieniono tylko offline audit surface.

## Verification

```bash
python3 -m py_compile scripts/shadow_v2_gatekeeper_coverage_denominator_audit.py tests/test_shadow_v2_gatekeeper_coverage_denominator_audit.py
python3 tests/test_shadow_v2_gatekeeper_coverage_denominator_audit.py
python3 scripts/shadow_v2_gatekeeper_coverage_denominator_audit.py --summary-csv reports/selector/shadow_v2_terminal_executable_pnl_smoke_pr41_summary.csv --output-csv reports/selector/shadow_v2_l2_e_gatekeeper_coverage_denominator_summary.csv --pretty
```

## Final Decision

```text
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
next_stage=repair_candidate_universe_denominator_before_l2_f
```
