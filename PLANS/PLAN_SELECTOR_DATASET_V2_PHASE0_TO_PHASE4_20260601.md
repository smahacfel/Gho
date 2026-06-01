# Plan Wykonawczy V2: Dataset i Selektor Pump.fun V1 dla Ghost

**Data zapisu:** 2026-06-01  
**Status:** Phase 0 w toku; FSC v2 capture/evidence lane praktycznie domykane, ale nie zamknięte jako pełna kwalifikacja providera ani jako aktywny sygnał polityki.

## 1. Cel

Celem jest zbudowanie bezpiecznego pipeline'u selektora dla pump.fun, który najpierw zamyka dataset, label, denominator i offline comparison, a dopiero później pozwala rozważać shadow-only score emit.

Obowiązkowa kolejność:

```text
Phase 0: evidence freeze + manifests + FSC capture/evidence qualification
Phase 1: accepted lifecycle + full candidate universe
Phase 2: feature snapshots + R1/R2 labels
Phase 3: selector training view + V2.5/V3/baseline comparison
Phase 4: reports + gates + optional shadow-only emit
```

Current Gatekeeper pozostaje baseline/candidate generator. Ten plan nie stroi Gatekeepera, nie zmienia BUY/REJECT/TIMEOUT, IWIM, execution, sendera ani lifecycle runtime.

Kanoniczny format v1:

```text
JSONL + manifest JSON
```

Parquet może być tylko opcjonalną projekcją, nie źródłem prawdy.

## 2. Aktualny Punkt Zatrzymania

Jesteśmy nadal przed Phase 1 selector dataset work. Bieżąca praca dotyczy Phase 0, a dokładniej wąskiej bramki:

```text
FSC v2 capture/evidence lane
```

To znaczy:

- FSC może być dołączany do datasetu jako evidence, jeśli przejdzie capture/evidence gates.
- FSC nie jest aktywnym sygnałem Gatekeepera.
- FSC nie jest hard rejectem.
- FSC nie jest size-down rule.
- FSC nie jest promotion-readiness claim.
- FSC nie zastępuje R2 canonical market path.

Aktualna interpretacja statusów:

```text
FSC_CAPTURE_EVIDENCE_12H_AUDITED = pending until 12h run completes
FSC_ACTIVE_POLICY = OFF
FSC_SCORING = NOT CLAIMED
FSC_PROVIDER_POLICY_QUALIFICATION = NOT CLAIMED unless eventual/postfill comparison is closed
```

Jeśli 12h capture/audit przejdzie, ale eventual/postfill comparison nadal będzie brakować, dopuszczalny status przed Phase 1 to:

```text
FSC_CAPTURE_EVIDENCE_12H_AUDITED = PASS
FSC_EVENTUAL_COMPARISON = PENDING
FSC_ACTIVE_POLICY = FORBIDDEN
```

Nie wolno wtedy opisywać stanu jako full PR8 provider qualification PASS.

## 3. Publiczne Artefakty i Kontrakty

Dataset artifacts pod:

```text
/root/Gho/datasets/selector/<scope>/
```

Wymagane pliki:

```text
candidate_universe_v1.jsonl
accepted_lifecycle_v1.jsonl
feature_snapshots_v1.jsonl
selector_training_view_v1.jsonl
```

Reports pod:

```text
/root/Gho/reports/selector/<scope>/
```

Wymagane pliki:

```text
dataset_manifest_v1.json
label_coverage_v1.json
gatekeeper_compare_v25_v3_v1.json
selector_baseline_v1.json
leakage_audit_v1.json
```

Offline scripts:

```text
/root/Gho/scripts/build_selector_dataset.py
/root/Gho/scripts/build_selector_candidate_universe.py
/root/Gho/scripts/build_selector_accepted_lifecycle.py
/root/Gho/scripts/build_selector_feature_snapshots.py
/root/Gho/scripts/build_selector_training_view.py
/root/Gho/scripts/compare_selector_gatekeepers.py
/root/Gho/scripts/train_selector_baseline.py
```

FSC capture/evidence scripts and reports are supporting Phase 0 evidence, not replacement for selector dataset artifacts.

## 4. Candidate Identity Contract

`candidate_id`:

- use existing `candidate_id`, gdy jest dostępny;
- inaczej deterministycznie:

```text
mint_id:bonding_curve_pubkey:birth_ts_ms
```

Każdy row musi mieć:

```text
candidate_id
candidate_id_source
```

Collision policy:

- fail closed;
- zero silent overwrite;
- identity collision musi trafić do manifestu i gate statusu.

Brak birth identity, quote identity albo timestamp completeness oznacza:

```text
universe_incomplete
```

Nie wolno domyślać brakujących elementów denominatora.

## 5. Candidate Universe Contract

`candidate_universe_v1` ma mieć jeden wiersz na każdy in-scope SOL-paired pump.fun bonding-curve birth/create event.

W universe muszą zostać również:

- REJECT;
- TIMEOUT;
- hard rejects;
- rows bez accepted lifecycle.

Universe nie może być budowany z accepted rows.

Universe nie może być budowany z decision logs jako denominator source. Decision logs mogą być co najwyżej context inputem.

V1 cohort:

```text
SOL-paired pump.fun bonding-curve launches
```

Schema ma pozostać quote-aware, ale v1 selector nie rozszerza universe na non-SOL.

Birth/create source dla Phase 1:

```text
existing Ghost canonical birth/create lane:
NewPoolDetected -> Candidate -> candidate_universe_v1
```

NLN `pumpfun.create` nie jest wymagane jako birth SSOT. Przy limicie 2 streamów NLN Pro poprawny układ jest:

```text
Ghost birth lane: NewPoolDetected / Candidate / pool lifecycle start
NLN Program Stream #1: prod.rpc.solana.system.transfers
NLN Program Stream #2: prod.rpc.solana.pumpfun.trade
```

`pumpfun.create` jest optional i nie może blokować FSC capture, ale brak canonical birth source blokuje Phase 1.

## 6. R2 Source of Truth Contract

R2 SSOT:

```text
Yellowstone/Geyser AccountUpdates
DIAG_ACCOUNT_UPDATE_RELAY
canonical account-state snapshots
```

RPC może być wyłącznie:

```text
flagged backfill/enrichment
```

RPC nie może mieszać się bez oznaczenia z canonical stream label. Źródło zawierające `rpc` nie może przechodzić jako canonical tylko dlatego, że nazwa zawiera też marker typu `canonical_account_state`.

NLN Program Streams nie są R2 SSOT. NLN może zasilać semantic capture/evidence lane, w szczególności FSC:

```text
system.transfers -> funding sources
pumpfun.trade -> buyer set
```

R2 mierzy market opportunity/token quality. FSC mierzy funding-source concentration evidence dla buyer cohort. To są osobne kontrakty.

## 7. Feature Snapshot Contract

`feature_snapshots_v1` musi być decision-time-safe.

Każdy feature row musi mieć:

```text
feature_cutoff_ts_ms
feature_cutoff_slot
feature_source
feature_observed_lag_ms
snapshot_kind
```

Dozwolone `snapshot_kind`:

```text
birth+5s
birth+15s
birth+30s
birth+60s
decision
```

Brak pełnego cutoff metadata oznacza:

```text
feature_snapshot_incomplete
```

Zakazane w `feature_snapshots_v1`:

```text
close_reason
final_pnl_pct
truth_status
execution outcome
labels
post-close fields
fields computed after feature_cutoff_ts_ms
```

Minimalne cechy v1:

```text
curve_progress_pct
net_quote_in_15s
net_quote_in_30s
trade_rate
unique_buyers
sell_share
top1_wallet_share
buyer_hhi
creator_sold_early_flag
quote_mint == SOL
```

FSC v2 może zostać dołączone jako evidence tylko jeśli zachowuje status, unknown/degraded reasons i coverage metadata. Nie wolno mapować:

```text
UNKNOWN -> clean 0.0
NEUTRAL_ONLY -> clean 0.0
low coverage -> organic positive
```

## 8. Label Contracts

### 8.1 R1

R1 mierzy realized lifecycle outcome.

`R1_positive`:

```text
truth_status = resolved
AND execution_realized = true
AND (
  close_reason = Target
  OR final_pnl_pct >= pnl_target_net_pct
)
```

`R1_negative`:

```text
truth_status = resolved
AND execution_realized = true
AND (
  close_reason in {StopLoss}
  OR final_pnl_pct <= 0
  OR (
    close_reason = TimeStop
    AND final_pnl_pct < pnl_target_net_pct
  )
)
```

Wymagane pola:

```text
r1_label
r1_label_reason
r1_excluded_reason
r1_gray_reason
```

TimeStop z dodatnim PnL poniżej targetu jest negatywny dla R1 target-label, ale musi mieć reason:

```text
time_stop_below_target
```

Gray/partial/execution-not-realized rows są wyłączone z R1 label denominatora.

### 8.2 R2

R2 mierzy market opportunity/token quality niezależnie od execution outcome.

`R2_positive`:

```text
target before stop inside horizon H
```

`R2_negative`:

```text
stop first
OR no target until H
```

Ale `no target until H` jest negative wyłącznie gdy:

```text
path_coverage_ok = true
AND horizon_matured = true
```

Następujące przypadki nie są negative:

```text
incomplete stream
restart gap
missing DIAG
censored path
horizon_unmatured
missing path
```

Mają trafić do:

```text
r2_status = censored | missing_path | stream_incomplete | horizon_unmatured
```

Parametry R2:

```text
target_net_pct
stop_net_pct
horizon_ms
```

muszą pochodzić z config/manifest albo być wymaganymi CLI parametrami. Zakaz ukrytych stałych.

## 9. Denominator Contract

Główna metryka:

```text
Precision_R2 = TP_R2 / (TP_R2 + FP_R2)
```

Literalny denominator:

```text
split = holdout
AND selector_accept = true
AND cohort_in_scope = true
AND stream_completeness_ok = true
AND label_resolved = true
AND r2_label in {positive, negative}
```

`execution_only_failure` nie wyklucza R2, chyba że narusza dostępność canonical market path. Wtedy row dostaje explicit unresolved/censored reason.

Wymagane denominator flags w `selector_training_view_v1`:

```text
eligible
label_resolved
cohort_in_scope
stream_completeness_ok
label_excluded_reason
execution_only_failure
split
selector_accept
```

Nie wolno raportować precision na ładnym subsetcie bez jawnego denominatora.

## 10. Phase 0: Evidence Freeze + Manifests + FSC Capture

### 10.1 Scope Phase 0

Zamrozić:

- existing lifecycle reporter output;
- Gatekeeper decisions;
- Seer/Yellowstone/OracleRuntime event artifacts;
- DIAG/account-update streams;
- config snapshot;
- artifact provenance;
- replay artifact version;
- NLN FSC capture/evidence artifacts, jeśli FSC ma być dołączone jako feature evidence.

Wymagany output:

```text
reports/selector/<scope>/dataset_manifest_v1.json
```

Manifest musi zawierać:

- input provenance;
- output provenance;
- config snapshot/hash;
- R2 SSOT contract;
- replay artifact version;
- stage reports;
- shadow-only emit disabled;
- explicit gates and fail reasons.

### 10.2 FSC Phase 0 Subgate

FSC v2 capture/evidence lane musi zostać domknięte tylko jako evidence lane.

Required streams przy limicie 2 NLN streams:

```text
prod.rpc.solana.system.transfers
prod.rpc.solana.pumpfun.trade
```

Optional:

```text
prod.rpc.solana.pumpfun.create
prod.rpc.solana.pumpfun.transaction
swaps
```

Runtime nie może próbować odpalać 3 streamów na limicie 2. Ma czytelnie degradować optional topic albo failować przed startem, bez cichej utraty danych.

FSC required acceptance before Phase 1 dataset inclusion:

```text
runtime duration >= 12h
nln_provider_benchmark_v1.status = PASS
fsc_coverage_v2.status = PASS
audit_rows > 0
shared_event_keys > 0
benchmark_duration_below_minimum absent
audit_rows_missing absent
normalization_error_rows classified by topic
raw_rows >= normalized_rows
fake_zero_fsc_count = 0
UNKNOWN/NEUTRAL not treated as clean 0.0
funding_source_v2 present in decision logs
Gatekeeper active FSC remains OFF
decision_enabled = false
hard_reject_enabled = false
no active BUY/REJECT/TIMEOUT drift
```

2026-06-01 amendment after PR8 r2 host-pressure finding:

The original 12h / independent provider benchmark gate remains the stronger
qualification contract for future active FSC scoring or full provider
qualification. It is no longer a blocker for Phase 1 dataset entry when all of
the following are true:

```text
FSC is capture/evidence only
provider_independent_benchmark = NOT_AVAILABLE / NOT_CLAIMED
no external audit feed exists for this run
fsc_capture_canary_v1.status = PASS
fsc_coverage_v2.status = PASS
fsc_unknown_reason_v2.status = PASS
nln_native_fsc_join_sanity_v1.status = PASS
fake_zero_fsc_count = 0
UNKNOWN/NEUTRAL/low coverage remain explicit, not clean 0.0
Gatekeeper active FSC remains OFF
decision_enabled = false
hard_reject_enabled = false
full provider qualification is NOT_CLAIMED
builder_scale_caveat is explicit if reports are windowed/bounded
```

If the FSC provider builder uses bounded/windowed transfer processing, the
manifest must say so via:

```text
parameter_grid_scope = full | windowed | sampled
transfer_processing_mode = streaming | full_scan | bounded_tail
builder_scale_caveat = true | false
```

Windowed/bounded FSC reports may support Phase 1 evidence inclusion only when
they do not claim provider completeness, full provider qualification, active
policy readiness, precision lift, R2 SSOT status or replacement for Phase 1
dataset artifacts.

Full PR8 provider qualification additionally requires:

```text
decision_time_vs_eventual_fsc_v1.status = PASS
comparison_rows > 0
```

If this additional comparison is missing, allowed status is only:

```text
FSC_CAPTURE_EVIDENCE_12H_AUDITED = PASS
FSC_EVENTUAL_COMPARISON = PENDING
FSC_ACTIVE_POLICY = FORBIDDEN
```

### 10.3 FSC Non-Goals

Do not claim:

- FSC active policy;
- FSC hard reject;
- FSC size-down;
- FSC precision lift;
- FSC promotion readiness;
- full provider qualification without eventual/postfill comparison.

### 10.4 Phase 0 Exit

Phase 0 can exit into Phase 1 only when:

```text
dataset_manifest_v1.json exists
Phase 0 input provenance is frozen
canonical birth/create source is identified and durable
R2 SSOT source contract is explicit
FSC capture/evidence status is explicit
shadow/live separation is explicit
no active policy mutation is claimed
```

## 11. Phase 1: Accepted Lifecycle + Candidate Universe

`accepted_lifecycle_v1` must be built as a projection of:

```text
/root/Gho/scripts/shadow_onchain_lifecycle_report.py
```

Do not build a new lifecycle labeler from scratch.

`candidate_universe_v1` must be built from the full SOL-only birth/create universe, not from accepted rows.

Hard rejects and TIMEOUT remain in universe for coverage/recall.

Join accepted lifecycle to universe must have:

```text
join_completeness >= 99%
```

If below 99%, Phase 1 status is NO-GO unless the manifest explicitly scopes and explains the missing cohort.

Phase 1 outputs:

```text
datasets/selector/<scope>/candidate_universe_v1.jsonl
datasets/selector/<scope>/accepted_lifecycle_v1.jsonl
reports/selector/<scope>/label_coverage_v1.json
```

## 12. Phase 2: Feature Snapshots + R1/R2 Labels

Build:

```text
feature_snapshots_v1.jsonl
```

Then build labels:

```text
R1 lifecycle outcome labels
R2 canonical market path labels
```

Phase 2 must enforce:

- feature cutoff metadata;
- no leakage;
- R2 canonical source provenance;
- censored/missing/horizon-unmatured classification;
- explicit label reasons;
- explicit excluded reasons.

Phase 2 outputs:

```text
datasets/selector/<scope>/feature_snapshots_v1.jsonl
reports/selector/<scope>/leakage_audit_v1.json
reports/selector/<scope>/label_coverage_v1.json
```

## 13. Phase 3: Training View + Offline V2.5/V3 + Baselines

Build:

```text
selector_training_view_v1.jsonl
```

This joins:

- universe;
- snapshots;
- R1/R2 labels;
- denominator flags;
- split metadata;
- optional FSC evidence status fields.

Comparison V2.5/V3/baseline must use:

```text
same candidate set
same label
same time split
same eligibility flags
same feature cutoff
same observation window
same replay artifact version
same accept-rate buckets
```

Required accept-rate buckets:

```text
native
1%
2.5%
5%
10%
```

If row has no replay input or raw score:

```text
replay_input_missing
```

No pseudo-score.

Baselines:

```text
rules baseline
regularized logistic regression
shallow gradient boosting
```

Split:

```text
temporal only
```

Threshold selection:

```text
validation only
```

Permutation importance:

```text
holdout only
```

Phase 3 outputs:

```text
datasets/selector/<scope>/selector_training_view_v1.jsonl
reports/selector/<scope>/gatekeeper_compare_v25_v3_v1.json
reports/selector/<scope>/selector_baseline_v1.json
```

## 14. Phase 4: Reports + Gates + Optional Shadow-Only Emit

Sample gates:

```text
first baseline >= 80-100 accepted BUY rows with resolved label
V2.5/V3 comparison >= 150 accepted resolved rows
V2.5/V3 comparison >= 1000 eligible candidates
feature importance preferably 150-300 accepted resolved rows
feature importance preferably 1000-5000 eligible candidates
```

Promotion gate:

```text
holdout Precision_R2 >= 0.70
leakage audit PASS
denominator counts explicit
holdout_accepted_count >= 50 for preliminary conclusion
holdout_accepted_count >= 100 before shadow-only emit/tuning
```

Optional shadow-only emit can be added only after gates.

Shadow-only emit must be:

- disabled by default;
- additive;
- guarded with `#[serde(default)]` for new Rust/log fields;
- no impact on active policy;
- no impact on IWIM;
- no impact on execution;
- no impact on lifecycle.

No live/P2 mutation is authorized by this plan.

## 15. Test Plan

Required Python checks:

```text
python3 -m py_compile scripts/build_selector_dataset.py
python3 -m py_compile scripts/build_selector_candidate_universe.py
python3 -m py_compile scripts/build_selector_accepted_lifecycle.py
python3 -m py_compile scripts/build_selector_feature_snapshots.py
python3 -m py_compile scripts/build_selector_training_view.py
python3 -m py_compile scripts/compare_selector_gatekeepers.py
python3 -m py_compile scripts/train_selector_baseline.py
python3 -m unittest scripts/test_selector_pipeline.py -v
```

Lifecycle contract tests:

```text
python3 -m unittest scripts/test_shadow_onchain_lifecycle_report_contract.py -v
```

Unit tests must cover:

- candidate dedupe;
- SOL-only cohort;
- missing quote/birth fail-closed;
- collision fail-closed;
- accepted lifecycle projection;
- R1 target/stop/timestop/non-positive/excluded/gray cases;
- R2 target-before-stop;
- R2 stop-before-target;
- R2 no-target with matured horizon;
- R2 censored/missing path/horizon-unmatured;
- literal Precision_R2 denominator;
- feature cutoff null fail-closed;
- leakage field exclusion;
- comparison parity by split/window/replay version;
- replay input missing as explicit status;
- manifest consistency.

FSC capture checks before Phase 1:

```text
raw_rows >= normalized_rows
normalization errors reported by topic
funding lane health fields populated or explicitly unavailable
nln_provider_benchmark_v1.status PASS for 12h capture gate
fsc_coverage_v2.status PASS
active FSC policy disabled
```

Rust tests only after runtime/log-schema/config changes. Do not run broad cargo tests for offline Python/reporting-only changes unless needed.

Final checks:

```text
git diff --check
manifest consistency
join completeness report
leakage audit PASS or explicit NO-GO
```

## 16. Assumptions and Non-Goals

Assumptions:

- V1 cohort is SOL-paired pump.fun bonding-curve launches.
- `MaterializedFeatureSet` and `PoolObservationSession::materialize_features()` remain SSOT for Gatekeeper decisions.
- Selector dataset is offline evidence/training view, not a competing active policy source.
- Existing lifecycle reporter remains source of accepted lifecycle truth.
- New pipeline performs projection, join and denominator discipline.

Non-goals:

- execution redesign;
- BCV2 spiral;
- Helius `transactionSubscribe` as primary ingest;
- live execution;
- Gatekeeper retune;
- legacy scoring revival;
- active FSC policy;
- FSC hard reject;
- FSC promotion readiness before offline proof;
- changing BUY/REJECT/TIMEOUT behavior during Phase 0-3.

## 17. Operational Sequence From Current State

Current next steps:

1. Let the active FSC 12h capture/audit run mature.
2. Verify:

```text
nln_provider_benchmark_v1.status
fsc_coverage_v2.status
decision_time_vs_eventual_fsc_v1.status
fsc_provider_qualification_manifest_v1.json
```

3. If 12h capture passes but eventual/postfill remains missing, mark:

```text
FSC_CAPTURE_EVIDENCE_12H_AUDITED = PASS
FSC_EVENTUAL_COMPARISON = PENDING
FSC_ACTIVE_POLICY = FORBIDDEN
```

4. Freeze Phase 0 manifests.
5. Move to Phase 1:

```text
candidate_universe_v1
accepted_lifecycle_v1
join completeness >= 99%
```

6. Do not start Phase 2 until Phase 1 universe and lifecycle join are formally gated.

## 18. Decision Summary

The plan is safe only if the order remains:

```text
dataset -> label -> denominator -> offline comparison -> baseline -> optional shadow-only emit
```

Do not tune Gatekeeper on an incomplete denominator.

Do not mix R1 realized execution outcome with R2 market opportunity.

Do not treat Program Streams, FSC, RPC backfill or decision logs as substitutes for canonical R2 market path.

Do not claim selector readiness until the public artifacts and gates exist under:

```text
datasets/selector/<scope>/
reports/selector/<scope>/
```
