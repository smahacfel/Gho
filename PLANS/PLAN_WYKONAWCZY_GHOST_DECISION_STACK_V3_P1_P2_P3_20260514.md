# Plan Wykonawczy Ghost Decision Stack V3 P1-P3

> Data: 2026-05-14  
> Status: plan wykonawczy do implementacji  
> Zakres: P1 calibrated shadow funnel, P2 ADR-gated selective promotion, P3 calibration gate  
> Repo baseline: `/root/Gho`, branch `main`  
> Zrodla: `V3.md`, `PLANS/PLAN_WYKONAWCZY_GHOST_DECISION_STACK_V3_P0_20260513.md`, clean P0 validation artifact

---

## 1. Summary

Po udanym P0 V3 shadow/evidence dalsza sciezka ma pozostac **shadow-first**.
P1 buduje kalibrowalny V3 staged funnel bez zmiany active policy. P2 opisuje
wylacznie **ADR-gated selective promotion** wybranych, zwalidowanych hard gates.
P3 jest **Calibration Gate**: replay/ablation/calibration certification, ktory
musi poprzedzic szersza promocje i nie moze uzywac live ML ani post-hoc labeli
jako runtime inputu.

Stan startowy:

- P0 jest GO na clean baseline `min_market_cap_sol = 30.0`.
- V3 dziala jako addytywny sidecar `v3_shadow_*`.
- `decision_plane` pozostaje `v25_shadow`.
- aktywny `reason_code_version` pozostaje `2`.
- brak podstaw do P2 promotion z obecnego runu: raport ma `v3_rows=141`, ale
  `execution.success_count=0` i brak BUY/lifecycle proof.

---

## 2. P1 - Calibrated Shadow Funnel

Cel: przeniesc P0 z minimalnego sidecaru na konfigurowalny, replayowalny V3
staged funnel, nadal bez active promotion.

### Implementacja

1. Dodac `GatekeeperV3Config`
   - Nowy modul: `ghost-brain/src/config/gatekeeper_v3_config.rs`.
   - Eksport przez `ghost-brain/src/config/mod.rs`.
   - Dodac do glownego configu jako `#[serde(default)] pub gatekeeper_v3:
     GatekeeperV3Config`.
   - TOML SSOT dla progow: `ghost-brain/ghost_brain_config.toml`, sekcja
     `[gatekeeper_v3]`.
   - `configs/rollout/shadow-burnin.toml` zostaje tylko rollout/artifact SSOT,
     bez duplikacji progow decyzyjnych.

2. Struktura `GatekeeperV3Config`
   - `enabled: bool`, default `false`.
   - `shadow_emit_enabled: bool`, default `false`.
   - `policy_version: u32`, default `1`.
   - `materialization_version: u32`, default `1`.
   - `early`, `normal`, `extended` jako osobne profile progow.
   - `evidence_requirements` dla feature groups: identity, curve, tx_intel,
     tx_segments, pdd_sequence, alpha, sybil, cpv, fsc, organic_broadening,
     manipulation_contradiction, execution.
   - `confidence_caps`: osobne capy dla unavailable/degraded/stale/fallback/
     not_configured oraz `execution_not_run`.
   - `component_weights`: opportunity/risk weights bez hardcodowania w
     `gatekeeper_v3.rs`.
   - `promotion` sekcja obecna, ale domyslnie disabled i niewykorzystywana w P1.

3. Przepiac evaluator V3 na config V3
   - Obecne `evaluate_v3_from_features(&MaterializedFeatureSet,
     &GatekeeperV2Config, deadline_elapsed)` rozszerzyc do wariantu przyjmujacego
     `&GatekeeperV3Config`.
   - Zachowac kompatybilny wrapper dla testow P0, jesli potrzebny.
   - Usunac P0 hardcoded constants jako zrodlo decyzji; moga zostac tylko jako
     defaults w configu.
   - Nie zmieniac aktywnego `GatekeeperBuffer::evaluate_from_features()` ani
     `evaluate_policy_from_assessment()`.

4. Dodac jawny model actionability
   - Nie dodawac globalnego `EvidenceStatus::is_actionable()`.
   - Dodac lokalna, V3-policy funkcje typu
     `evaluate_feature_actionability(group, status, stage, config)`.
   - Actionability ma zalezec od feature group i stage.
   - Missing/degraded evidence nie moze dawac BUY ani clean pass.

5. Dodac hash evidence
   - `v3_policy_config_hash`: hash kanonicznie serializowanego
     `GatekeeperV3Config`.
   - `v3_feature_snapshot_hash`: hash kanonicznie serializowanego
     `MaterializedFeatureSet`.
   - `v3_materialization_version`: wersja materializacji uzyta w hash payload.
   - Nie zastepowac istniejacego `config_hash`, ktory nadal oznacza obecny
     V2/V2.5 routing/config hash.

6. Rozszerzyc DecisionLogger addytywnie
   - Dodac pola `Option<T>` z
     `#[serde(default, skip_serializing_if = "Option::is_none")]`:
     - `v3_policy_config_hash`
     - `v3_feature_snapshot_hash`
     - `v3_materialization_version`
     - `v3_policy_version`
     - `v3_stage_thresholds`
     - `v3_component_scores`
     - `v3_actionability`
   - Nie tworzyc `decision_plane = "v3_shadow"`.
   - Nie nadpisywac `reason_code`, `verdict_type`, `decision_verdict_buy`.

7. Rozszerzyc raporty
   - Rozszerzyc `scripts/v3_shadow_report.py` o:
     - hash presence/consistency
     - per-stage threshold distribution
     - component score buckets
     - actionability summary
     - config hash matrix
     - snapshot hash uniqueness/duplicate diagnostics
   - Dodac testy do `scripts/test_v3_shadow_report.py`.

### Acceptance P1

- Stare TOML-e laduja sie bez `[gatekeeper_v3]`.
- Stare JSONL v20 bez nowych pol nadal sie parsują.
- Ten sam V3 config daje ten sam `v3_policy_config_hash`.
- Ten sam snapshot daje ten sam `v3_feature_snapshot_hash`.
- V3 evaluator pozostaje pure: tylko snapshot + config + deadline context.
- `v3_shadow_*` nadal jest sidecarem, nie routed plane.
- Raport V3 pokazuje `status=ok`, `v3_rows>0`, hash coverage > 0.
- Active verdict fields nie zmieniaja sie wzgledem P0.

---

## 3. P2 - ADR-Gated Selective Promotion

Cel: opisac warunkowa promocje tylko tych V3 hard gates, ktore przejda P1/P3
evidence. P2 nie jest automatycznym wdrozeniem po P1.

### Warunki wejscia

P2 moze ruszyc tylko jesli:

- P1 raporty maja stabilne hash/replay evidence.
- P3 Calibration Gate potwierdzi replay parity i brak istotnego overfittingu.
- Istnieje formalny ADR promotion.
- Jest osobny rollback flag dla kazdego promowanego gate.
- Jest rozdzielony decision quality proof od execution feasibility proof.

### Kandydaci do promocji

Promowac wolno tylko hard gates, ktore sa:

- deterministic,
- SSOT-based,
- config-gated,
- replay-validated,
- typed-reason-coded,
- odwracalne flaga.

Kolejnosc kandydatow:

1. Identity/protocol contradiction.
2. Curve untradable / stale unsafe only with clean evidence.
3. PDD entry drift with strong anchor.
4. PDD ramping/whale/reserve with sufficient sample.
5. Manipulation contradiction only after ablation proof.
6. Sybil/FSC/CPV combo only after strong evidence and separate analysis.

Nie promowac w P2:

- Early BUY.
- V3 confidence-only BUY.
- execution infeasibility gate bez execution-path review.
- IWIM ordering changes.
- ML/calibrator output jako live input.

### Implementacja P2

1. Dodac promotion flags do `GatekeeperV3Config`
   - `promotion.enabled = false`.
   - Per-gate flags, np. `promote_identity_gate`, `promote_curve_gate`,
     `promote_pdd_entry_drift`.
   - Kazdy flag ma `require_adr = true`.
   - Kazdy flag ma `rollback_enabled = true`.

2. Wpiac selective gates w active policy tylko przez explicit bridge
   - Bridge musi mieszkac przy active policy boundary, nie w loggerze.
   - Bridge konsumuje `MaterializedFeatureSet` i `GatekeeperV3Config`.
   - Bridge moze emitowac tylko wybrane hard rejecti, nie BUY.
   - Active reason code musi byc typed i jawnie mapowany.
   - V3 sidecar nadal loguje pelny shadow verdict niezaleznie od promotion.

3. Logging P2
   - Dodac:
     - `v3_promoted_gate`
     - `v3_promotion_adr_id`
     - `v3_promotion_decision`
     - `v3_promotion_reason_code`
     - `v3_promotion_rollback_flag`
   - Jesli gate nie jest promowany, pola pozostaja `None`.

4. Report P2
   - Dodac `scripts/v3_promotion_report.py` albo rozszerzyc raport V3 trybem
     `--promotion`.
   - Raport musi rozdzielac:
     - V3 shadow verdict,
     - V3 promoted gate,
     - active verdict,
     - execution/shadow lifecycle outcome,
     - unknown/missing status.

### Acceptance P2

- Bez ADR i flag zaden V3 gate nie wplywa na active policy.
- Promowany gate nie moze emitowac generic reject.
- Promowany gate nie moze tworzyc BUY.
- Promowany gate nie moze czytac raw tx ani runtime mutable state.
- Rollback przez config przywraca zachowanie V2/V2.5.
- Raport pokazuje zero `decision_plane = "v3_shadow"`.
- Test active invariance przechodzi przy wszystkich promotion flags disabled.

---

## 4. P3 - Calibration Gate

Cel: stworzyc twardy gate dowodowy dla kalibracji confidence, replay parity,
ablation i jakosci decyzyjnej przed szersza promocja. P3 nie wdraza live ML i
nie uzywa outcome labeli jako runtime features.

### Dane wejsciowe

P3 korzysta z:

- V3 JSONL sidecar rows.
- `v3_policy_config_hash`.
- `v3_feature_snapshot_hash`.
- `v3_materialization_version`.
- shadow lifecycle / simulation evidence, jesli istnieje.
- post-decision labels tylko offline.
- active vs V2.5 vs V3 verdict matrix.
- confidence buckets.
- degraded evidence distribution.

### Implementacja P3

1. Dodac replay/ablation runner
   - Nowy skrypt: `scripts/v3_replay_ablation_report.py`.
   - Wejscia:
     - `--config`
     - `--decisions-log`
     - `--shadow-lifecycle`
     - `--events-dir`
     - `--json`
   - Output:
     - replay parity status,
     - missing hash status,
     - confidence calibration buckets,
     - ablation deltas per component,
     - false BUY proxy,
     - false REJECT opportunity proxy,
     - timeout quality taxonomy.

2. Replay parity
   - Dla kazdego row z `v3_feature_snapshot_hash` raport sprawdza:
     - czy hash istnieje,
     - czy policy hash istnieje,
     - czy V3 verdict jest powtarzalny przy tym samym snapshot/config,
     - czy duplicate `ab_record_id` nie zmienia verdictu.
   - Jesli snapshot payload nie jest jeszcze w pelni rekonstruowalny z JSONL,
     raport ma zwracac `replay_status = "hash_only"` zamiast falszywego OK.

3. Ablation
   - Minimalne warianty:
     - full V3,
     - no organic broadening,
     - no manipulation contradiction,
     - no sybil/FSC/CPV caps,
     - no alpha cap,
     - no execution cap.
   - Ablation nie moze zmieniac live configu.
   - Wynik to raport offline, nie policy patch.

4. Calibration
   - Bucketowac `v3_shadow_confidence`:
     - `0`
     - `0_to_0_25`
     - `0_25_to_0_50`
     - `0_50_to_0_75`
     - `0_75_to_1_00`
   - Kazdy bucket musi miec:
     - count,
     - active verdict distribution,
     - V3 verdict distribution,
     - outcome label coverage,
     - unknown outcome ratio,
     - degraded evidence ratio.
   - Nie wolno deklarowac jakosci procentowej przy niskim label coverage.

5. Promotion certification
   - P3 generuje machine-readable certification:
     - `p3_status = pass | fail | insufficient_data`
     - `promotion_ready_gates = []`
     - `blocked_gates = []`
     - `insufficient_evidence_gates = []`
   - Domyslny wynik przy braku BUY/lifecycle labels: `insufficient_data`, nie
     fail kodu.

### Acceptance P3

- Raport nie traktuje `submitted`, `missing`, `no_dispatch`, `unknown` jako
  success.
- Replay parity nie udaje pelnego replay, jesli dostepny jest tylko hash.
- Ablation rozdziela decision-quality od execution-quality.
- Confidence calibration nie uzywa przyszlych danych jako runtime features.
- P3 moze rekomendowac P2 ADR dla konkretnych gates, ale nie aktywuje ich.

---

## 5. Test Plan

P1 targeted tests:

```bash
cargo test -p ghost-core materialized
cargo test -p ghost-core feature_builder
cargo test -p ghost-brain gatekeeper_v3_config
cargo test -p ghost-brain decision_logger
cargo test -p ghost-brain reason_code
cargo test -p ghost-launcher gatekeeper_v3
cargo test -p ghost-launcher v3_shadow
python3 -m unittest scripts/test_v3_shadow_report.py
```

P2 targeted tests:

```bash
cargo test -p ghost-launcher gatekeeper_v3
cargo test -p ghost-launcher gatekeeper_policy
cargo test -p ghost-launcher --test gatekeeper_v25_regression
cargo test -p ghost-brain decision_logger
python3 -m unittest scripts/test_v3_shadow_report.py
```

P3 targeted tests:

```bash
python3 -m unittest scripts/test_v3_shadow_report.py
python3 -m unittest scripts/test_v3_replay_ablation_report.py
python3 scripts/v3_shadow_report.py --config configs/rollout/shadow-burnin.toml --json
python3 scripts/v3_replay_ablation_report.py --config configs/rollout/shadow-burnin.toml --json
```

Optional final repo gates:

```bash
cargo fmt --check
cargo test --workspace
```

If `cargo fmt --check` or workspace tests fail from known unrelated drift, record
exact files/failures and do not treat them as P1/P2/P3-specific unless they touch
changed paths.

---

## 6. Rollout Order

1. P1.1: config structs and defaults only.
2. P1.2: V3 evaluator consumes `GatekeeperV3Config`.
3. P1.3: hashes and additive logger fields.
4. P1.4: report expansion.
5. P1.5: clean shadow rerun with P1 hashes.
6. P3.1: replay/ablation report in hash-only mode.
7. P3.2: calibration buckets and label coverage.
8. P3.3: certification output.
9. P2.1: ADR template and config flags only.
10. P2.2: disabled-by-default promotion bridge.
11. P2.3: one selected hard gate behind ADR/config after P3 pass.

---

## 7. Non-Negotiable Boundaries

- No P2 active promotion during P1.
- No P3 live ML.
- No V3 BUY promotion in this plan.
- No `decision_plane = "v3_shadow"`.
- No change to IWIM ordering.
- No change to live sender, blockhash, retries, DirectBuyBuilder,
  DirectSellBuilder.
- No raw tx reads in `gatekeeper_v3.rs`.
- No global `EvidenceStatus::is_actionable()`.
- No treating missing/degraded evidence as clean.
- No treating shadow simulation as live inclusion.
- No generic reject replacing typed reason code.
- No HyperPrediction/Chaos/legacy score revival.

---

## 8. Assumptions

- P3 is defined as `Calibration Gate (Recommended)`, per user decision.
- P2 is `ADR-gated (Recommended)`, per user decision.
- `GatekeeperV3Config` belongs in `ghost-brain` config, not rollout TOML.
- Rollout TOML remains artifact/runtime SSOT, not policy-threshold SSOT.
- Existing `config_hash` remains V2/V2.5 hash; V3 gets separate
  `v3_policy_config_hash`.
- P1 can be implemented without changing active verdict behavior.
- P2 cannot begin until P1/P3 evidence is available.
- Current P0 artifact is sufficient as baseline, but not sufficient for
  promotion.

---

## 9. Delegation Trace

```yaml
delegation_trace:
  task_classification: "cross-cutting V3 P1-P3 execution plan"
  routing_performed: true
  primary_specialist: "Ghost Runtime Coordinator"
  supporting_specialists_considered:
    - "Config Rollout Safety Reviewer"
    - "Decision Logging Replay Analyst"
    - "Gatekeeper Policy Auditor"
    - "SSOT Feature Materialization Guardian"
    - "Statistical Research Engine"
    - "Large Data Analytics"
  specialist_docs_loaded:
    - "docs/agents/ghost-runtime-coordinator.md"
    - "docs/agents/config-rollout-safety-reviewer.md"
    - "docs/agents/decision-logging-replay-analyst.md"
    - "docs/agents/gatekeeper-policy-auditor.md"
  skills_used:
    - "ghost-execution"
    - "statistical-research-engine"
    - "large-data-analytics"
  subagents_used:
    - "V3 post-P0 implementation mapping"
    - "config/report/replay artifact mapping"
  fast_path_used: false
  contracts_checked:
    - "MaterializedFeatureSet SSOT"
    - "PoolObservationSession materialization boundary"
    - "Gatekeeper active verdict separation"
    - "DecisionLogger additive schema"
    - "shadow/live boundary"
    - "config backward compatibility"
    - "replay/audit evidence"
    - "typed reason-code preservation"
  unresolved_routing_uncertainty: []
```
