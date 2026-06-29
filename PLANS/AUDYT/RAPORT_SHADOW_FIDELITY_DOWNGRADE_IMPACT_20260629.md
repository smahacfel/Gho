# Raport Shadow Fidelity Downgrade Impact 2026-06-29

## 1. Final measurement verdict

Finalny verdict pomiarowy:

```text
SHADOW_REPLAY_LIFECYCLE_MISMATCH
```

Ten downgrade pack jest konsekwencja audytu P0 Shadow Burnin Fidelity. Nie zmienia runtime, BUY/REJECT, Gatekeeper policy, selector runtime, TX/Jito/live path, `shadow_close_only` ani active close.

## 2. Decyzja

Obecny shadow burnin nie moze byc traktowany jako live-equivalent ani jako jedna spojna prawda pozycji laczaca `shadow_exit_replay_v1` i `shadow_lifecycle`.

`shadow_exit_replay_v1` moze pozostac komponentowym offline path-label evidence, ale tylko pod jawnie ograniczonymi zalozeniami i bez claimow live fill / live sell / live PnL.

## 3. Konsekwencje obowiazkowe

- shadow is not live-equivalent;
- shadow entry price is not proven as live fill;
- shadow exit result is offline path/label evidence, not executable sell fill;
- replay/lifecycle cannot be treated as one unified position truth;
- 2s/3s conclusions are sparse approximation only;
- 300s/500s conclusions are not evaluable;
- live runtime approval remains false;
- shadow_close_only approval remains false;
- active close approval remains false;
- R51 is ACTIVE_PARTIAL / DIAGNOSTIC_ONLY, not strategy evidence.

## 4. Downgrade old strategy reports

### ORG-A0

ORG-A0 remains no-runtime, but only under offline path-label measurement assumptions.

Downgrade label:

```text
DOWNGRADE_SHADOW_NOT_LIVE_EQUIVALENT
DOWNGRADE_REPLAY_LIFECYCLE_MISMATCH
```

ORG-A0 nie moze byc cytowany jako proof live PnL, executable fills, live slippage behavior ani real landing outcome.

### R48/R2 exit matrix

R48/R2 exit matrix remains no-runtime, but not live-equivalent.

Downgrade label:

```text
DOWNGRADE_EXIT_FILL_NOT_PROVEN
DOWNGRADE_HORIZON_COVERAGE_NOT_PROVEN
```

R48/R2 moze byc uzywany tylko jako komponentowy replay/path-label material, nie jako lifecycle/live truth.

### TSV2 A1/A2/A3

TSV2 A1/A2/A3 remains diagnostic only; lifecycle/replay mismatch blocks active close proof.

Downgrade label:

```text
DOWNGRADE_REPLAY_LIFECYCLE_MISMATCH
DOWNGRADE_TEMPORAL_LABEL_FEATURE_SEPARATION_UNPROVEN
```

TSV2 nie dostaje active close approval na podstawie shadow fidelity audit.

### EIX

EIX remains data-blocked.

Downgrade label:

```text
DOWNGRADE_ENTRY_FILL_NOT_PROVEN
DOWNGRADE_EXIT_FILL_NOT_PROVEN
```

Brak live-equivalent entry/exit fill proof blokuje podniesienie EIX ponad status data-blocked.

### RTP-A0

RTP-A0 remains diagnostic only; not live-equivalent.

Downgrade label:

```text
DOWNGRADE_SHADOW_NOT_LIVE_EQUIVALENT
DOWNGRADE_REPLAY_LIFECYCLE_MISMATCH
```

RTP-A0 nie moze byc uzyty jako dowod realnego runtime PnL.

### RUG-MARKUP-A0

RUG-MARKUP-A0 remains no-runtime under component replay, not a live fill proof.

Downgrade label:

```text
DOWNGRADE_ENTRY_FILL_NOT_PROVEN
DOWNGRADE_EXIT_FILL_NOT_PROVEN
```

RUG-MARKUP-A0 moze pozostac analiza offline, ale nie dowodzi live slippage, executable fill ani landing outcome.

### RCE-A0

RCE-A0 remains blocked by missing surface; R51 cannot be interpreted until shadow fidelity issues are explicitly bounded.

Downgrade label:

```text
DOWNGRADE_SHADOW_NOT_LIVE_EQUIVALENT
DOWNGRADE_REPLAY_LIFECYCLE_MISMATCH
DOWNGRADE_R51_ACTIVE_PARTIAL_DIAGNOSTIC_ONLY
```

R51 jest `ACTIVE_PARTIAL / DIAGNOSTIC_ONLY`, nie strategy evidence. Nie wolno uzywac R51 jako RCE proof ani jako runtime approval proof.

## 5. Explicit no-citation rule

Previous reports must not be cited as proof of live PnL, executable fills, live slippage behavior, or real landing outcome.

To obejmuje takze cytowanie posrednie: jezeli downstream raport opiera sie na ORG-A0, R48/R2, TSV2 A1/A2/A3, EIX, RTP-A0, RUG-MARKUP-A0 albo RCE-A0, musi przeniesc downgrade label i nie moze usuwac ograniczen shadow fidelity.

## 6. Approval status after downgrade

```yaml
final_measurement_verdict: SHADOW_REPLAY_LIFECYCLE_MISMATCH
live_equivalent: false
runtime_approval: false
shadow_close_only_approval: false
active_close_approval: false
r51_status: ACTIVE_PARTIAL_DIAGNOSTIC_ONLY
old_reports_live_pnl_proof: false
old_reports_executable_fill_proof: false
old_reports_real_landing_outcome_proof: false
```

## 7. Required language for future references

When citing old strategy reports, use wording no stronger than:

```text
This is offline shadow/path-label evidence under downgraded measurement assumptions.
It is not live-equivalent, not executable fill proof, not live slippage proof,
and not real landing outcome proof.
```

## 8. Required next instrumentation before upgrade

Any upgrade from this downgrade state requires evidence for:

- exact entry quote/min_out/reserve-before/reserve-after/decimals;
- submit timestamp and submit-to-land latency;
- actual landing slot or failed/no-fill status;
- exit quote/min_out/slippage/fees/own sell impact;
- per-path sample slot/timestamp/commitment;
- exact tie-break metadata for same-slot target/stop;
- lifecycle/replay exact join id and terminal-event cardinality.

Until these are instrumented and re-audited, live runtime approval, shadow_close_only approval and active close approval remain false.
