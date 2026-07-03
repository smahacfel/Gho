# RAPORT SHADOW V2 L2 RESEARCH-GRADE READINESS AUDIT 20260703

## 1. Werdykt wykonawczy

Final verdict:

```text
L2_RESEARCH_GRADE_PATH_PRESENT
```

Shadow V2 po domknięciu L1 ma działający deterministic diagnostic execution simulation:

- entry L1 diagnostic fill działa w realnym shadow flow;
- exit L1 diagnostic fill działa w realnym shadow flow;
- terminal executable PnL jest generowany;
- complete executable diagnostic roundtrip istnieje;
- manifest/shutdown/replay-lifecycle podstawowo działają dla smoke scope.

To nadal **nie jest L2 research-grade**. Obecne evidence zatrzymuje się na `DIAGNOSTIC_SIM`, ponieważ `RESEARCH_CANDIDATE` blokują głównie:

- niepełny chain ordering w `EventOrderKey`;
- brak `account_data_hash` w runtime boundary pool-state provenance;
- brak research-evaluable path density dla wymaganych horyzontów;
- smoke-size, który nie jest próbką badawczą;
- brak pełnego research validation run z audytami PASS.

Ścieżka do L2 jest jednak obecna i nie wymaga L3 live calibration. L3 pozostaje osobnym poziomem: live-confirmed fills, realized slippage, quote/fill divergence, landing/failure telemetry i calibration dataset.

Approval flags pozostają:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
```

## 2. Zakres i źródła dowodowe

Zakres PR43-A0 jest wyłącznie audit/report-only. Nie wykonano burnina, nie zmieniono runtime, nie zmieniono BUY/REJECT, Gatekeeper policy, selector runtime, TX/Jito/live path, R51, `shadow_close_only` ani active close.

Punkt startowy na `main`:

```text
main_head=ab81807dee0d9e62106822808982be25d0cb804b
PR42_merge_commit=ab81807dee0d9e62106822808982be25d0cb804b
PR42_verdict=PR41_TERMINAL_EXECUTABLE_PNL_SMOKE_PASS
```

Główne źródła:

- `reports/selector/shadow_v2_terminal_executable_pnl_smoke_pr41_summary.json`
- `scripts/shadow_v2_temporal_no_lookahead_audit.py`
- `scripts/shadow_v2_path_density_horizon_audit.py`
- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`
- `ghost-brain/src/guardian/post_buy/shadow_v2_execution.rs`
- `ghost-core/src/account_state_core/types.rs`
- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-launcher/src/components/post_buy_runtime.rs`

## 3. Aktualny L1 baseline

PR42 smoke potwierdził:

```text
accepted_shadow_handoff_count=28
entry_fill_FILLED_count=28
exit_fill_FILLED_count=28
entry_FILLED_exit_FILLED_same_position_count=28
terminal_truth_with_final_pnl_executable_bps_count=28
complete_executable_roundtrip_positions=28
entry_execution_label_grade_DIAGNOSTIC_SIM_count=28
exit_execution_label_grade_DIAGNOSTIC_SIM_count=28
entry/exit execution_label_grade_RESEARCH_CANDIDATE_count=0
entry/exit execution_label_grade_LIVE_CONFIRMED_count=0
```

Wynik jest wystarczający dla L1 diagnostic deterministic simulation smoke, ale nie dla L2. L2 musi przejść z `DIAGNOSTIC_SIM` do `RESEARCH_CANDIDATE` na podstawie pełniejszej provenance i ordering, a nie tylko deterministycznej formuły fill.

## 4. Temporal / no-lookahead

Aktualny temporal audit dla PR41 scope:

```text
temporal_audit_verdict=BLOCKED_TEMPORAL_AMBIGUITY_REMAINS
event_order_key_present_rows=252
event_order_key_exempt_rows=29
event_order_key_missing_required_rows=0
non_monotonic_event_seq_in_process=0
post_entry_fields_used_in_pre_decision_context=0
terminal_truth_used_as_pre_entry_evidence=0
derived_replay_lifecycle_used_as_canonical_input=0
```

To oznacza: nie ma wykrytej twardej leakage ani missing required `event_order_key`, ale pozostaje ambiguity przez jawne `UNKNOWN` w chain order.

Rozbicie explicit UNKNOWN:

```text
block_time=252
transaction_index_or_unknown=252
instruction_index_or_unknown=252
inner_instruction_index_or_unknown=252
log_index_or_unknown=252
signature=168
```

Ambiguity dotyczy przede wszystkim pool-state, path/exit/terminal ordering, czyli pól, które L2 musi traktować jako causal evidence. `event_seq_in_process` wystarcza do wewnętrznego uporządkowania runtime, ale sam nie jest chain-order proof research-grade.

## 5. Account data hash / provenance

`PoolStateSampleV2::research_blockers()` wymaga `account_data_hash`. Obecny runtime boundary nie dostarcza hash:

- `CanonicalPoolState` przechowuje zdekodowane reserves, slot i timestamp, ale nie raw account bytes ani hash;
- `AccountStateUpdate` przechowuje reserves, slot, write_version, receive timestamp/seq/source, ale nie raw account bytes ani hash;
- `ShadowV2EntryBoundaryPayload` ma `account_data_hash: Option<String>`, ale capture w `TriggerComponent` wpisuje `None`;
- testy mogą tworzyć hash fixture, ale runtime boundary go nie niesie.

Brak `account_data_hash` **nie blokuje L1 diagnostic fill**, bo deterministic execution może policzyć quote/fill z reserves. Blokuje natomiast `research_provenance_ready=true`, a więc blokuje `RESEARCH_CANDIDATE`.

Minimalny L2 contract:

- hash powinien być liczony z raw account bytes z ingest path, najlepiej BLAKE3 zgodnie z istniejącym helperem `account_data_hash_blake3`;
- jeśli raw bytes nie mogą być utrzymane w stanie, wystarczy trwały hash przeniesiony w `AccountStateUpdate` / `CanonicalPoolState` / boundary payload;
- decoded-state hash może być dodatkowy, ale nie zastępuje raw account hash, jeśli claim brzmi "to jest hash account data";
- brak hash pozostaje provenance blockerem, nie execution blockerem.

## 6. EventOrderKey research-grade contract

Obecny `EventOrderKey` zawiera:

```text
slot
block_time
signature
transaction_index_or_unknown
instruction_index_or_unknown
inner_instruction_index_or_unknown
log_index_or_unknown
event_seq_in_process
observed_at_wall_ms
```

L2 research-grade ordering wymaga co najmniej:

- known slot dla każdego ordering-sensitive canonical event;
- non-empty signature tam, gdzie event jest związany z chain event/transaction;
- tx index, instruction index, inner instruction index i log index albo jawnie udowodnioną politykę, że dany event nie wymaga intra-tx ordering;
- `observed_at_wall_ms > 0`;
- monotonic `event_seq_in_process` w zadeklarowanym scope;
- brak same-slot ambiguity dla boundary, które decydują o fill/terminal truth.

Ocena obszarów:

- Entry pool state before vs entry fill: L1 działa diagnostycznie, ale L2 wymaga hash i pełniejszego order key boundary.
- Exit pool state before vs exit fill: L1 działa diagnostycznie, ale L2 wymaga hash i pełniejszego order key boundary.
- Terminal truth vs entry/exit fills: terminal executable PnL działa, ale L2 musi utrzymać exact links oraz order key bez silent fallback.
- Derived pool state after: może pozostać deterministic derived, ale musi być oznaczone jako derived i nie może udawać observed account state.
- Path samples: obecny smoke nie dostarcza research-evaluable density; L2 musi mieć path samples z coverage i ordering dla wymaganych horyzontów.

## 7. Density / horizon evaluability

Aktualny path density audit:

```text
path_density_audit_verdict=BLOCKED_DENSITY_NOT_EVALUABLE_FOR_REQUIRED_HORIZONS
density_rows=1967
horizons=2000,3000,10000,30000,120000,300000,500000
EVALUABLE_EXACT=0
EVALUABLE_APPROX=0
SPARSE_APPROX_ONLY=0
NOT_EVALUABLE_NO_COVERAGE=791
NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY=1176
path_points_median=1
path_points_max=1
coverage_points_median=1
coverage_points_max=1
```

Problem nie jest live-equivalence. Problem jest badawczy: L2 nie może wnioskować o target/stop/timeout/path behavior na horyzontach, których replay nie pokrywa.

15-min smoke nie jest właściwym narzędziem do domknięcia density. Smoke może potwierdzać wiring i shutdown, ale L2 density wymaga osobnego validation/research-run z:

- skonfigurowanym horyzontem replay;
- retencją path samples;
- więcej niż jednym path point per position;
- jasnym rozdzieleniem unsupported horizons jako `NOT_EVALUABLE`;
- raportem density per horizon.

## 8. ResearchCandidate grade checklist

`execution_label_grade=RESEARCH_CANDIDATE` może zostać uznane dopiero, gdy dla danej pozycji/event family spełnione są warunki:

```text
execution_simulation_ready=true
research_provenance_ready=true
pool_state_sample_v2.research_blockers()=[]
account_data_hash present
reserves/token_decimals/sol_lamports present
staleness_ms and staleness_slots present
event_order_key complete enough for no-lookahead boundary
temporal audit PASS for scope
manifest/shutdown PASS
replay/lifecycle reconciliation PASS
terminal executable PnL present for roundtrip claims
density evaluable for horizons claimed in research
sample size adequate for stated research question
```

Nie wymagamy L3 live calibration do samego L2. L2 może pozostać offline deterministic research-grade, jeśli zachowuje causal boundary, ordering, provenance, density i reconstruction contracts.

## 9. Sample size / offline research readiness

PR41 smoke ma tylko 28 complete executable roundtrips. To wystarcza na smoke PASS, nie na research conclusion.

Minimalne wymagania L2 offline research powinny zostać zdefiniowane w PR46 validation plan:

- osobny research validation run, nie smoke;
- minimalna liczba accepted shadow positions i complete executable roundtrips zależna od pytania badawczego;
- oddzielna klasyfikacja normal/organic/rug/synthetic/scam profiles;
- candidate universe i denominator jawnie zapisane;
- unsupported horizons oznaczane `NOT_EVALUABLE`;
- holdout/walk-forward split może być wymagany dopiero dla strategy research, ale L2 musi przygotować evidence tak, żeby taki split był możliwy.

Proponowane minimalne gates dla pierwszego L2 readiness validation:

```text
complete_executable_roundtrip_positions >= 500
entry/exit RESEARCH_CANDIDATE count > 0
temporal_audit_verdict=PASS_TEMPORAL_NO_LOOKAHEAD_AUDIT
path_density_audit_verdict=PASS_DENSITY_EVALUABLE_FOR_REQUIRED_HORIZONS for claimed horizons
manifest_retention_audit=PASS
replay_lifecycle_reconciliation=PASS
malformed canonical rows=0
unknown_or_untyped_blockers=0
```

Liczby są acceptance seed, nie finalny strategy proof.

## 10. L2 vs L3 boundary

L2 nie rozwiązuje i nie musi rozwiązywać:

- real live-confirmed fills;
- realized live slippage;
- measured quote/fill divergence;
- live failed/no-fill tx telemetry;
- Jito tip / priority fee / bundle success calibration;
- landing latency model;
- land/no-land/failure model calibrated on real txs;
- live-confirmed calibration dataset.

Te elementy blokują `LIVE_EQUIVALENCE`, nie samo L2 offline research-grade, o ile raporty L2 nie udają executable live fill.

## 11. Blocker matrix summary

Macierz blockerów:

```text
artifact=reports/selector/shadow_v2_l2_research_grade_blocker_matrix.csv
rows=19
L2_blockers=13
L3_only_blockers=5
final_verdict=L2_RESEARCH_GRADE_PATH_PRESENT
```

Główne L2 blockery:

- temporal chain-order UNKNOWN components;
- missing account data hash;
- raw account hash not carried by AccountStateCore boundary;
- density not evaluable;
- smoke sample size not research-grade;
- zero `RESEARCH_CANDIDATE` fills under current provenance state.

Główne L3-only blockers:

- live confirmed fills;
- realized slippage;
- quote/fill divergence;
- landing/failure telemetry;
- live calibration dataset;
- Jito/priority execution calibration.

## 12. Minimalny plan PR43-B / PR44+

### PR43-B — EventOrderKey research provenance wiring

Zakres:

- przenieść dostępne chain-order components z ingest/runtime boundary do `EventOrderKey`;
- uzupełnić signature, tx index, instruction index, inner instruction index, log index, block_time, gdy źródło je posiada;
- utrzymać explicit `UNKNOWN`, gdy źródło ich nie posiada;
- nie używać `event_seq_in_process` jako substytutu chain order w L2;
- nie zmieniać BUY/REJECT, Gatekeeper, selector, TX/Jito/live.

Acceptance:

```text
event_order_key_missing_required_rows=0
non_monotonic_event_seq_in_process=0
same-slot ambiguity materially reduced or typed
temporal audit no hard leakage
```

### PR44 — Account data hash provenance

Zakres:

- dodać hash raw account bytes w ingest/reducer boundary;
- przenieść hash do `AccountStateUpdate` / `CanonicalPoolState` lub shadow boundary payload;
- `PoolStateSampleV2.account_data_hash` ma pochodzić z hash raw account data, nie z fake value;
- opcjonalnie dodać decoded-state hash jako osobne pole/limitation, nie jako zamiennik raw hash.

Acceptance:

```text
POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME_count=0 for boundary samples where raw bytes are available
research_provenance_ready can become true when all other blockers are absent
```

### PR45 — L2 path density/horizon contract

Zakres:

- skonfigurować research validation path mode dla wymaganych horyzontów;
- utrzymać sample retention i replay horizon;
- dodać raport density per horizon;
- nie wnioskować o 300s/500s bez coverage.

Acceptance:

```text
PASS_DENSITY_EVALUABLE_FOR_REQUIRED_HORIZONS for claimed horizons
unsupported horizons marked NOT_EVALUABLE
```

### PR46 — L2 research validation run and audit pack

Zakres:

- osobny, dłuższy validation/research run;
- nie smoke;
- audyty entry/exit reconstruction, temporal, density, manifest, replay/lifecycle;
- sample-size gates;
- final verdict tylko dla offline research readiness, nie runtime approval.

Acceptance:

```text
entry/exit RESEARCH_CANDIDATE count > 0
complete executable roundtrips meet sample gate
temporal/density/manifest/replay lifecycle PASS
research_grade_candidate=true_for_shadow_v2_offline_research_only
runtime_approval=false
live_equivalence=false
```

### PR47+ — L3 live-equivalence calibration

Zakres:

- live-confirmed dataset;
- landing/failure/no-fill telemetry;
- realized slippage and quote/fill divergence;
- model error bounds.

To jest poza L2.

## 13. Decyzja końcowa

```text
L2 path: present
L2 status today: not granted
L2 blockers: known and engineering-solvable without L3
L3 live-equivalence: not granted and not required for L2
runtime approval: false
strategy research unblocked: false
recommended_next_pr: PR43-B EventOrderKey research provenance wiring
```

PR43-A0 nie odblokowuje research-grade. PR43-A0 tylko stwierdza, że istnieje realistyczna, ograniczona ścieżka do L2 bez mieszania jej z L3 live calibration.
