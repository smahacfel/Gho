# ADR-8D: Shadow V2 L2-F R2 Ultra Relaxed Validation Config 20260705

## Status

Local validation run configuration created and started.

## Decision

Uruchomiono osobny L2-F collection scope:

```text
run_id=shadow-v2-l2-f-collection-20260705-r2
launcher_config=configs/rollout/shadow-v2-l2-f-collection-20260705-r2.local.toml
ghost_brain_config=configs/rollout/ghost_brain_shadow_v2_l2_f_collection_20260705_r2.local.toml
configured_run_seconds=21600
```

Profil R2 jest run-local i sluzy tylko do zebrania walidacyjnego materialu
L2-F. Nie zmienia kodu runtime decisions, provider streams, BUY/REJECT policy,
selector runtime, TX/Jito/live path, shadow close ani active close.

## Context

Pierwszy L2-F collection scope mial zbyt mala liczbe kompletnych diagnostic
roundtripow i nadal nie mogl dowiesc L2-F gates. R2 ma zwiekszyc przepustowosc
shadow validation sampling bez grantowania approval flags.

Operator wskazal jawne minimalne wartosci:

```text
min_total_tx=3
min_unique_signers=2
min_buys=2
```

R2 dodatkowo poluzowal aktywne run-local min/max thresholds tak, aby zebrac
szerszy walidacyjny scope. To jest sampling profile, nie policy promotion.

## Config Boundary

Launcher pozostaje w shadow-only runtime:

```text
entry_mode=shadow_only
execution_mode=shadow
max_concurrent_positions=25
```

Ghost Brain approval flags pozostaja false:

```text
live_execution_enabled=false
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
```

Gatekeeper validation minima w profilu R2:

```text
min_tx_count=3
min_unique_signers=2
min_buy_count=2
min_sol_threshold=0.0
```

## Non-Goals

R2 nie grantuje:

```text
runtime_approval
research_grade
live_equivalence
strategy_research_unblocked
shadow_close_only
active_close
```

R2 nie zmienia:

```text
Gatekeeper policy code
BUY/REJECT code
selector runtime
TX/Jito/live path
provider streams
production defaults
```

## Consequences

1. R2 moze zebrac wiecej entry/exit diagnostic records niz R1.
2. R2 nadal musi przejsc L2-F audits po zakonczeniu runa.
3. Pozytywny maksymalny werdykt pozostaje ograniczony do:

```text
L2_RESEARCH_GRADE_CANDIDATE_OFFLINE_ONLY
```

4. Brak kompletnego runa albo brak ktoregokolwiek wymaganego gate ma byc
   raportowany fail-closed jako blocker.

## Verification

Przed startem R2 wykonano:

```bash
cargo build --release -p ghost-launcher
target/release/ghost-launcher --config configs/rollout/shadow-v2-l2-f-collection-20260705-r2.local.toml --preflight
```

Preflight potwierdzil:

```text
entry_mode=shadow_only
execution_mode=shadow
min_tx=3
min_unique=2
min_buy=2
preflight: all runtime checks passed
```

## Final Decision

```text
l2_f_r2_collection_scope_started=true
runtime_decision_behavior_changes=NONE
run_local_validation_config_changes=YES
provider_stream_changes=NONE
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
```
