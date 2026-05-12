---
name: Gatekeeper V25 Shadow Repair
overview: Plan naprawczy doprowadzający Gatekeeper V2.5 + shadow-burnin do stanu wiarygodnej selekcji pooli. 7 priorytetów P0-P6 nad istniejącym repair stream'em z 2026-05-05, bez naruszania SSOT/boundary contracts.
todos:
  - id: p0-dow-timer
    content: "P0: DOW timer per pool — gwarantowane okna Early/Normal/Extended niezależnie od ruchu TX, usunięcie unreachable! w try_shadow_evaluate(Extended)"
    status: pending
  - id: p1-segment-sequence
    content: "P1: Dodanie Option<TxSegmentSequence> do MaterializedFeatureSet (N1 zachowany), Path B liczy TAS + spike/ramping/flash z sequence z honest unavailable_reason"
    status: pending
  - id: p2-aps-path-b
    content: "P2: APS w evaluate_policy_from_assessment, regime_local_heuristic_enabled jako provisional flag, HighVolatility drift override w shadow plane (B1 zachowany)"
    status: pending
  - id: p3-drift-cap
    content: "P3: max_price_change_ratio z 9999.0 → 1.50 + ADR (świadome zamknięcie blind spot, stopniowe zaostrzanie)"
    status: pending
  - id: p4-reason-code
    content: "P4: GatekeeperReasonCode typed enum, schema bump v16→v17, 100% reason_code completeness (BUY/REJECT/TIMEOUT), 3 podtypy TIMEOUT"
    status: pending
  - id: p5-shadow-lifecycle
    content: "P5: payer_strategy=ephemeral dla shadow_only (B6), idempotency_key, shadow_lifecycle.jsonl enforcement, eventbus backpressure alarm"
    status: pending
  - id: p6-validation
    content: "P6: 17 testów kontraktowych, clean rollout shadow-burnin-v25-repair-r2 24h, walidator GO, ADR promotion readiness"
    status: pending
isProject: false
---

# Plan naprawczy Gatekeeper V2.5 + shadow-burnin

Pełny plan zapisany w: [PLANS/PLAN_NAPRAWCZY_GATEKEEPER_V25_SHADOW_BURNIN_20260507.md](PLANS/PLAN_NAPRAWCZY_GATEKEEPER_V25_SHADOW_BURNIN_20260507.md)

## Diagnoza warstwowa (audyt 2026-05-07)

Shadow-burnin nie jest wiarygodną symulacją scoringu pool/tokenów. 7 problemów warstwowych:

1. **V2.5 doczepione do `mode = "long"`** — nie jest pierwszoklasową ścieżką, znika gdy mode się zmieni ([gatekeeper.rs:3589](ghost-launcher/src/components/gatekeeper.rs))
2. **Okna DOW są tx-triggered** — Early/Normal nie odpalają się bez ruchu TX; Extended jest `unreachable!()` w `try_shadow_evaluate` ([gatekeeper.rs:3596-3617, 5606-5609](ghost-launcher/src/components/gatekeeper.rs))
3. **Path B (kanoniczny) nie widzi TAS/spike/ramping/flash** — `MaterializedFeatureSet` nie niesie `segment_sequence`; logi: `tas_unavailable_reason=materialized_features_missing_segment_sequence` ([gatekeeper_policy.rs:606-664](ghost-launcher/src/components/gatekeeper_policy.rs))
4. **APS jest telemetry-only** — `has_sufficient_history = false` hardcoded ⇒ regime zawsze `Normal`; APS w ogóle nie odpala w Path B ([gatekeeper_adaptive_prosperity.rs:97](ghost-launcher/src/components/gatekeeper_adaptive_prosperity.rs))
5. `**max_price_change_ratio = 9999.0`** — legacy drift cap wyłączony ([ghost_brain_config.toml:90](ghost-brain/ghost_brain_config.toml))
6. **TIMEOUT z `decision_reason = null`** — 2077/2614 rekordów latest scope; brak typed `reason_code` enum ([decision_logger.rs:196, 597-609](ghost-brain/src/oracle/decision_logger.rs))
7. **Shadow lifecycle nie domyka cyklu** — 0 BUY/0 entries w 2614 v25 decyzjach; `eventbus_lag_total = 5.5M`; `simulation_mismatch / ConstraintSeeds`

## Mapa priorytetów

```mermaid
flowchart LR
    P0[P0 DOW timing] --> P1[P1 SSOT segment_sequence]
    P0 --> P5[P5 Shadow lifecycle]
    P1 --> P2[P2 APS w Path B]
    P1 --> P3[P3 Legacy drift cap]
    P3 --> P4[P4 Reason code taxonomy]
    P2 --> P4
    P4 --> P5
    P5 --> P6[P6 Validation + clean rollout]
```



## Hard guardrails (niezmiennicze)

- **N1-N16 SSOT contracts** zachowane — w szczególności N14 (no synthetic parity) i N16 (no `GatekeeperMode::V25`)
- **B1-B8 boundary decisions** z istniejącego [PLANS/GATEKEEPER_V25_REPAIR_PLAN.md](PLANS/GATEKEEPER_V25_REPAIR_PLAN.md) zachowane
- **Legacy live plane** niezmieniony; V2.5 plane wciąż shadow-first (`live_execution_enabled = false`)
- **HyperPrediction Oracle** poza zakresem
- **PnL nie jest DoD** — DoD to coverage, invariants, audytowalność

## Final acceptance

17 testów kontraktowych zielone + clean rollout `shadow-burnin-v25-repair-r2` przez 24h + walidator NO-GO → GO + 100% `reason_code` completeness + shadow lifecycle domknięty + zero invariant violations.