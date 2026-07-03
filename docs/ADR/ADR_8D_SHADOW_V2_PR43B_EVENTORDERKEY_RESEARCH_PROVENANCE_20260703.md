# ADR-8D: Shadow V2 PR43-B EventOrderKey Research Provenance 20260703

## Status

Accepted as PR43-B implementation candidate.

## Decision

PR43-B wprowadza jawniejszy kontrakt `EventOrderKey` dla Shadow V2 L2 research provenance.

Final verdict:

```text
PR43B_EVENTORDERKEY_PROVENANCE_IMPROVED
```

Decyzja nie przyznaje:

```text
runtime_approval=true
shadow_close_only_approval=true
active_close_approval=true
research_grade=true
live_equivalence=true
strategy_research_unblocked=true
```

## Context

PR43-A0 ustalił:

```text
L2_RESEARCH_GRADE_PATH_PRESENT
```

L1 deterministic diagnostic execution simulation jest domknięty, ale L2 research-grade blokują:

- incomplete `EventOrderKey` chain order;
- brak `account_data_hash`;
- path density;
- sample size;
- `RESEARCH_CANDIDATE=0`.

PR43-B dotyczy tylko pierwszej klasy: `EventOrderKey` / temporal / no-lookahead research provenance.

Baseline temporal blocker:

```text
temporal_audit_verdict=BLOCKED_TEMPORAL_AMBIGUITY_REMAINS
explicit_unknown_chain_order_components:
  block_time=252
  transaction_index_or_unknown=252
  instruction_index_or_unknown=252
  inner_instruction_index_or_unknown=252
  log_index_or_unknown=252
  signature=168
```

## Evidence

Kod PR43-B:

- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`
- `ghost-brain/src/guardian/post_buy/engine.rs`
- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-launcher/src/components/post_buy_runtime.rs`
- `scripts/shadow_v2_temporal_no_lookahead_audit.py`

Artefakty PR43-B:

- `PLANS/AUDYT/RAPORT_SHADOW_V2_PR43B_EVENTORDERKEY_RESEARCH_PROVENANCE_20260703.md`
- `reports/selector/shadow_v2_pr43b_event_order_key_delta_summary.csv`

## Implementation

`EventOrderComponent<T>` rozróżnia teraz:

```text
UNKNOWN
NOT_APPLICABLE
DERIVED
RUNTIME_LOCAL
```

Wprowadzone zachowania:

- available source chain-order components są propagowane do `EventOrderKey`;
- brakujące source components pozostają explicit `UNKNOWN`;
- derived terminal truth nie udaje observed chain event;
- `event_seq_in_process` nie jest traktowany jako L2 chain-order proof;
- temporal audit rozdziela true unknown od derived/not-applicable/runtime-local classifications.

## Runtime Boundary

PR43-B nie zmienia:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- R51;
- `shadow_close_only`;
- active close.

PR43-B nie uruchamia burnina.

Runtime boundary remains:

```text
Shadow V2 evidence only
No decision consumption
No live execution implication
No approval flags granted
```

## Research Boundary

PR43-B poprawia provenance, ale nie domyka L2.

L2 nadal wymaga:

- runtime source for full chain order where required;
- `account_data_hash`;
- path density evaluability;
- sample-size gate;
- `RESEARCH_CANDIDATE` validation;
- manifest/replay/temporal audits PASS on an L2 validation scope.

## Rejected Alternatives

### Fake chain order values

Rejected. Brak signature, tx index, instruction index, inner instruction index, log index lub block time musi pozostać explicit `UNKNOWN`, jeżeli runtime source ich nie ma.

### Use `event_seq_in_process` as L2 proof

Rejected. `event_seq_in_process` jest runtime-local ordering aid. Nie dowodzi chain order ani same-slot tie-breakera dla research-grade no-lookahead.

### Reuse post-buy signature as pool-state source signature

Rejected. Entry pool-state boundary sample może dostać signature tylko ze swojego boundary source. Handoff/entry signature nie może udawać signature observed pool-state sample.

### Grant L2 from diagnostic L1 evidence

Rejected. Diagnostic deterministic fill może być poprawny formułowo, ale nadal nie mieć research provenance.

## Validation

Wykonane checks:

```text
python3 -m py_compile scripts/shadow_v2_temporal_no_lookahead_audit.py
cargo test -p ghost-brain shadow_v2_event_order -- --nocapture
cargo test -p ghost-launcher shadow_v2_event_order -- --nocapture
cargo test -p ghost-launcher shadow_v2_no_decision_consumption_static_guard -- --nocapture
cargo check -p ghost-brain
cargo check -p ghost-launcher
cargo fmt --check
git diff --check
git diff --cached --check
forbidden staged-file guard
```

## Consequences

1. Event ordering ambiguity is better classified.
2. Runtime sources can now propagate chain-order fields when available.
3. Missing fields remain visible blockers for L2.
4. Derived records are not silently treated as chain-observed records.
5. PR43-B requires a follow-up 15-minute smoke to measure runtime temporal delta.
6. L2 research-grade remains not granted.

## Required Follow-Up

Immediate follow-up after merge:

```text
PR43-C: 15-minute temporal delta smoke
```

Implementation follow-ups:

```text
PR44: account_data_hash provenance
PR45: path density / horizon evaluability
PR46: L2 research validation run
PR47+: L3 live-confirmed calibration
```

## Approval Flags

All remain false:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
```
