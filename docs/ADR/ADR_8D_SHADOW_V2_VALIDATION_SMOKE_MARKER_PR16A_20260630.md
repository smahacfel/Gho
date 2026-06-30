# ADR-8D: Shadow V2 Deterministic Validation Smoke Marker PR16A

Data: 2026-06-30

Status:

```text
PR16A_IMPLEMENTED_LOCAL_PENDING_REVIEW
```

## D1. Problem

PR16 smoke report udowodnil, ze PR15 harness potrafi przejsc preflight i
zestawic stream, ale nie potrafi deterministycznie wyprodukowac wymaganych
artefaktow V2, jezeli w krotkim smoke nie wystapi `BUY` ani accepted shadow
handoff.

Blokada z PR16:

```text
FAIL_BLOCKED_NO_CANONICAL_V2_EVIDENCE
```

Brak canonical eventu oznaczal brak:

- `shadow_position_event_v2.jsonl`;
- `shadow_replay_v2.jsonl`;
- `shadow_lifecycle_v2.jsonl`;
- `shadow_path_density_v2.jsonl`;
- `post_run_manifest.status=PASS`.

Smoke harness nie powinien zalezec od losowego zdarzenia strategii.

## D2. Decision

Dodano deterministic logging-only validation smoke marker emitowany przez
`PostBuyRuntime` po inicjalizacji `ShadowV2ValidationHarness`.

Marker jest zapisywany jako istniejący typ:

```text
ShadowPositionV2
```

ale z kontraktem diagnostycznym:

```text
simulation_level=MARK_ONLY
measurement_grade=DIAGNOSTIC_ONLY
temporal_class=UNKNOWN
quality=VALIDATION_SMOKE_MARKER_BLOCKED_BY_DATA
```

Marker jest aktywny tylko gdy:

```text
shadow_v2_burnin.enabled=true
shadow_v2_burnin.logging_only=true
```

Marker przechodzi przez ten sam `ShadowV2ValidationHarness::append_record()`,
ktory zapisuje canonical JSONL i derived replay/lifecycle/density snapshots.

## D3. Evidence

Zmiany kodowe:

- `ghost-launcher/src/components/post_buy_runtime.rs`

Zmiany kontraktowe:

- `docs/SPEC/SHADOW_BURNIN_V2_SIMULATION_CONTRACT_20260629.md`
- `reports/selector/shadow_v2_acceptance_gates.csv`

Nowy test:

```text
cargo test -p ghost-launcher shadow_v2_validation_smoke_marker_writes_required_artifacts_without_handoff -- --nocapture
```

Test potwierdza, ze bez event bus, bez `BUY`, bez `PostBuySubmitted` i bez
accepted shadow handoff powstaja:

```text
shadow_position_event_v2.jsonl rows=1
shadow_replay_v2.jsonl rows=1
shadow_lifecycle_v2.jsonl rows=1
shadow_path_density_v2.jsonl rows=7
```

Density rows maja verdict:

```text
NOT_EVALUABLE_NO_COVERAGE
```

co jest oczekiwane, bo marker nie tworzy path samples.

## D4. Root Cause

PR15 minimal emitter byl podpiety tylko pod accepted shadow handoff. To bylo
poprawne jako minimalne runtime-adjacent wiring, ale zle jako smoke gate, bo
krotki smoke mogl nie wygenerowac zadnego `BUY`.

Brak `BUY` nie powinien blokowac testu writer/materializer/manifest wiring.

## D5. Corrective Action

Wprowadzono:

- prywatny helper `maybe_emit_shadow_v2_validation_smoke_marker`;
- emisje markera raz po starcie `PostBuyRuntime` i inicjalizacji harnessu;
- unikalny `position_id` / `event_id` oparty o `run_namespace` i wall-clock, aby
  restart nie kolidowal z existing JSONL index;
- limitations:
  - `VALIDATION_SMOKE_MARKER_V2`;
  - `DIAGNOSTIC_ONLY_NOT_STRATEGY_POSITION`;
  - `BLOCKED_BY_DATA_NO_ENTRY_FILL_EXIT_FILL_OR_PATH`;
  - `NOT_CONSUMED_BY_DECISIONS`;
  - `NOT_STRATEGY_EVIDENCE`;
  - `NOT_LIVE_EQUIVALENT`;
  - `NO_BUY_REJECT_CHANGE`.

## D6. Rejected Alternatives

Odrzucono:

- czekanie w smoke na losowy `BUY`;
- luzowanie Gatekeeper policy lub BUY/REJECT tylko po to, zeby wywolac handoff;
- generowanie fake entry fill, exit fill, terminal truth albo path samples;
- nowy competing truth path poza `ShadowV2ValidationHarness`;
- podlaczenie markera do Gatekeeper, selector, TX/Jito/live path,
  `shadow_close_only` albo active close.

## D7. Consequences

PR16A nie przyznaje:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
strategy_research_unblocked=false
research_grade=not_granted
live_equivalence=not_granted
```

Po PR16A nastepny smoke moze sprawdzic writer/materializer/manifest wiring bez
czekania na realny accepted shadow handoff.

PR17 fidelity validation burnin pozostaje zablokowany, dopoki nowy smoke nie
potwierdzi:

```text
shadow_position_event_v2 rows > 0
shadow_replay_v2 rows > 0
shadow_lifecycle_v2 rows > 0
shadow_path_density_v2 rows > 0
post_run_manifest.status = PASS
clean shutdown proven
```

## D8. Verification

Wymagane sprawdzenia:

```text
cargo test -p ghost-launcher shadow_v2_validation_smoke_marker_writes_required_artifacts_without_handoff -- --nocapture
cargo test -p ghost-launcher shadow_v2_no_decision_consumption_static_guard -- --nocapture
cargo test -p ghost-launcher shadow_v2_manifest_audit_not_invoked_from_event_path -- --nocapture
cargo fmt --check
git diff --check
git diff --cached --check
forbidden staged-file guard
```

Runtime boundary:

```text
NO_BUY_REJECT_CHANGE
NO_GATEKEEPER_POLICY_CHANGE
NO_SELECTOR_RUNTIME_CHANGE
NO_TX_JITO_LIVE_PATH_CHANGE
NO_SHADOW_CLOSE_ONLY_ENABLEMENT
NO_ACTIVE_CLOSE_ENABLEMENT
NO_R51_TOUCH
NO_RAW_JSONL_STAGED
```
