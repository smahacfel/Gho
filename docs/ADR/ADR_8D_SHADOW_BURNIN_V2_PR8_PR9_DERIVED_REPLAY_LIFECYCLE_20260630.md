# ADR-8D: Shadow Burnin V2 PR8/PR9 Derived Replay i Lifecycle

Data: 2026-06-30

Status:

```text
PR8_PR9_COMPLETED_ON_PR_BRANCH_PENDING_REVIEW
```

## D1. Problem

P0 Shadow Burnin Fidelity Audit wykazał:

```text
SHADOW_REPLAY_LIFECYCLE_MISMATCH
```

W Shadow V1 `shadow_lifecycle` i `shadow_exit_replay_v1` zachowywały się jak
konkurencyjne prawdy pozycji. To blokowało wiarygodne traktowanie close reason,
close age i final PnL jako jednej terminalnej historii pozycji.

## D2. Decision

PR8/PR9 wprowadzają tylko inert, side-by-side typy i helpery projekcji:

- `ShadowReplayV2::derive_from_canonical_stream`;
- `ShadowLifecycleV2::derive_from_canonical_stream`;
- `reconcile_replay_lifecycle_v2`;
- `ShadowLifecycleEventTypeV2`;
- `ShadowReplayLifecycleReconciliationV2`.

`shadow_replay_v2` i `shadow_lifecycle_v2` są widokami pochodnymi z
`shadow_position_event_v2`. Nie są niezależnym źródłem prawdy. Jedyną
kanoniczną terminalną prawdą pozostaje rekord `shadow_terminal_truth_v2` w
canonical event stream.

## D3. Evidence

Kod:

- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`

Kontrakt:

- `docs/SPEC/SHADOW_BURNIN_V2_SIMULATION_CONTRACT_20260629.md`
- `reports/selector/shadow_v2_required_schema_manifest.csv`
- `reports/selector/shadow_v2_acceptance_gates.csv`
- `reports/selector/shadow_v2_remediation_workbreakdown.csv`
- `reports/selector/shadow_v2_risk_register.csv`

Testy lokalne:

```text
cargo test -q -p ghost-brain shadow_v2_replay
result: ok; 2 passed

cargo test -q -p ghost-brain shadow_v2_lifecycle
result: ok; 2 passed
```

## D4. Root Cause

V1 miał oddzielne artefakty replay/lifecycle bez wystarczającego
event-sourced terminal truth. To pozwalało na rozjazd terminalnego PnL, close
reason i close age oraz wymuszało fallback joins lub ręczną interpretację.

## D5. Corrective Action

PR8:

- generuje `shadow_replay_v2` z canonical event stream;
- zachowuje `canonical_event_stream_ref`;
- przenosi `source_event_ids`;
- przenosi `canonical_terminal_event_id`;
- rozdziela mark path lane od static executable lane;
- dodaje limitations: `REPLAY_V2_DERIVED_VIEW_NOT_CANONICAL_TRUTH` oraz
  `MARK_REPLAY_NOT_EXECUTABLE_FILL`.

PR9:

- generuje `shadow_lifecycle_v2` z tego samego canonical event stream;
- lifecycle jest `LifecycleSubEvent`, nie `TerminalTruth`;
- derived lifecycle nie zużywa one-terminal invariant;
- reconciliation używa wyłącznie exact join key:
  `run_id`, `session_id`, `position_id`, `pool_id`, `base_mint`;
- fallback join jest jawnie nieakceptowany.

## D6. Rejected Alternatives

Odrzucono:

- naprawianie `shadow_exit_replay_v1` jako prawdy terminalnej;
- traktowanie `shadow_lifecycle` V1 jako prawdy terminalnej;
- fallback join po `pool_id` albo `base_mint`;
- ciche scalanie replay/lifecycle mimo różnego exact join key;
- uznanie lifecycle sub-event za drugi terminal truth;
- podłączenie PR8/PR9 do runtime writerów lub live close path.

## D7. Consequences

Po PR8/PR9:

- replay/lifecycle V2 mogą być porównywane by construction, bo pochodzą z tego
  samego event stream;
- duplicate terminal truth pozostaje blokowane przez canonical stream;
- lifecycle sub-event nie jest terminalną prawdą;
- mark replay nadal nie jest executable sell fill;
- static executable lane nadal nie jest live-confirmed;
- finalny research-grade wymaga jeszcze PR10-PR12 i walidacyjnego burninu
  fidelity, reconciliation, density oraz manifests.

Granice pozostają:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
strategy_research_unblocked=false
```

## D8. Verification

Nowe lub rozszerzone testy:

- `shadow_v2_replay_derive_from_canonical_stream_separates_lanes`;
- `shadow_v2_lifecycle_derive_from_same_canonical_terminal_truth`;
- `shadow_v2_lifecycle_sub_event_does_not_create_duplicate_terminal_truth`;
- `shadow_v2_replay_lifecycle_reconciliation_uses_exact_join_only`.

Weryfikacja lokalna na branchu PR8/PR9:

```text
cargo test -q -p ghost-brain shadow_v2_replay
result: ok; 2 passed

cargo test -q -p ghost-brain shadow_v2_lifecycle
result: ok; 2 passed
```

Runtime boundary:

```text
NO_RUNTIME_SEMANTICS_CHANGED
NO_BUY_REJECT_CHANGE
NO_GATEKEEPER_POLICY_CHANGE
NO_SELECTOR_RUNTIME_CHANGE
NO_TX_JITO_LIVE_PATH_CHANGE
NO_SHADOW_CLOSE_ONLY_ENABLEMENT
NO_ACTIVE_CLOSE_ENABLEMENT
NO_RUN_STARTED
NO_R51_TOUCH
```
