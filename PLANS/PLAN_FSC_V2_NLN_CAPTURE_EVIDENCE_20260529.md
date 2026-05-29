# Plan FSC v2 NLN Capture/Evidence

**Date:** 2026-05-29
**Status:** Ready for implementation
**Primary objective:** uruchomic FSC jako evidence/capture lane z NLN Program Streams, bez aktywnego wplywu na BUY/REJECT.
**P0 artifact:** `docs/ADR/ADR-0138-fsc-v2-nln-program-streams-capture-evidence.md`

## 1. Executive Decision

Ten plan scala dwa wnioski:

1. Obecny kod repo ma realne fundamenty FSC: rolling index, dust threshold, neutral funding sources, warmup, capy i fail-closed degraded reasons.
2. Obecna semantyka FSC nie jest wystarczajaca do aktywnego sygnalu decyzyjnego: wzor jest legacy, attribution wybiera latest inbound, same-slot ordering moze spasc do timestamp/arrival fallback, coverage nie jest domkniete decyzyjnie, a stare `funding_source_concentration` nie moze po cichu dostac nowej semantyki.

Decyzja wykonawcza:

- **GO teraz:** NLN Program Streams jako additive FSC v2 capture/evidence lane.
- **GO teraz:** `FscV2Evidence` w datasetach, logach, materialized snapshot i offline selector pipeline.
- **NO-GO teraz:** active Gatekeeper BUY/REJECT, penalty, size-down, combo veto, hard reject i promotion readiness oparte o FSC.
- **GO pozniej:** soft scoring tylko po provider benchmarku, leakage audit, coverage report i offline ablation wzgledem R1/R2 labels.

FSC v2 jest featurem wejsciowym, nie labelem i nie R2 path source. Odpowiada na pytanie, czy early buyer set wyglada na skoordynowany fundingowo przez wspolne zrodlo native SOL. Nie odpowiada na pytanie, czy token spelnil market-opportunity outcome.

## 2. Architectural Boundaries

- `MaterializedFeatureSet` pozostaje kanonicznym snapshotem decyzji.
- `PoolObservationSession::materialize_features()` pozostaje granica materializacji.
- Policy nie dostaje bezposredniego dostepu do `FundingSourceIndex`.
- Program Streams sa semantic/program event layer dla `pumpfun.create`, `pumpfun.trade` i `system.transfers`.
- R2 canonical market path pozostaje raw Yellowstone AccountUpdates, DIAG albo canonical account-state snapshots.
- NLN RPC nie jest primary audit/replay backend dla FSC coverage.
- `grpc_global_stream` pozostaje filtered lane i nie moze zostac awansowany do `full_chain_coverage=true`.
- Stare `SybilResistanceFeatures.funding_source_concentration` pozostaje legacy surface; FSC v2 wymaga osobnego payloadu i osobnych guardow.

## 3. Current Repo Facts to Preserve

- `FundingSourceIndex` utrzymuje rolling state i zapisuje funding transfer observations.
- `observe_transfer()` odrzuca dust, puste/self transfery i rozgrzewa stan tylko przy `full_chain_coverage=true`.
- `compute_for_transactions()` materializuje legacy FSC do `MaterializedFeatureSet.sybil_resistance.funding_source_concentration`.
- Legacy formula to `1.0 - distinct_known_sources / known_sources.len()`, nie sample-normalized HHI.
- `lookup_source_for_buy()` wybiera latest eligible pre-buy transfer po reverse history scan.
- Same-signature ordering korzysta z instruction/event provenance, ale same-slot cross-signature moze spasc do event/arrival timestamps.
- `unique_successful_buyers()` deduplikuje buyerow przed sortowaniem, wiec przy out-of-order buforze moze wybrac niepierwszy buy.
- V3 `has_hard_risk_contradiction()` moze uzyc FSC przez `manipulation_contradictions.funding_source_concentration`, wiec capture FSC wymaga jawnego policy guardu.
- Obecne V3 primary-only profile trzymaja FSC praktycznie wylaczone: `fsc=false`, soft penalties 0, sybil interference layer off, combo veto off, `funding_lane_mode="disabled"`.

## 4. FSC v2 Definition

FSC v2:

```text
sample-normalized HHI po dominant meaningful non-neutral native-SOL funding source,
liczony dla first successful buy per unique buyer,
z transfer-before-buy, conservative same-slot ordering, coverage/warmup/lane-health,
neutral handling, dust abs/relative filtering, attribution confidence,
decision-time snapshot i wersjonowaniem metryki.
```

Buyer cohort:

```text
B = first successful buy per unique buyer
where buyer = pumpfun.trade.user albo canonical buyer identity z PoolTransaction
where ix_name == "buy" / is_buy == true
where buy is within observation/decision window
```

Funding candidate for buyer `b`:

```text
t.to_wallet == b
t.asset == NativeSol
t.amount passes storage + attribution dust policy
t is strictly before first_buy(b)
first_buy_ts - t.ts <= funding_lookback_window
t was available to the decision-time snapshot when snapshot_mode == DecisionTime
t is not invalid/self-transfer/malformed
```

Attribution scope:

```rust
pub enum FscAttributionScope {
    SingleHopNativeSol,
}
```

FSC v2 nie udaje pelnego graph lineage. Nie rozwiazuje multi-hop funding, mixerow, CEX withdrawal trees, bardzo starego finansowania ani walletow z historycznym saldem.

## 5. Attribution v2

Obecne latest eligible pre-buy transfer wins trzeba zastapic dominant meaningful source:

```text
For each buyer b:
  group valid candidate transfers by from_wallet
  source_amount[source] = sum(amount_lamports)
  total_candidate_amount = sum(all valid candidate amount_lamports)
  selected_source = argmax(source_amount[source])
  attribution_confidence = selected_source_amount / total_candidate_amount
```

Default:

- `fsc_min_attribution_confidence = 0.60`.
- Ponizej progu buyer nie jest czystym known non-neutral source.
- Status buyer attribution: `LowAttributionConfidence`.

Proponowane typy:

```rust
pub struct FundingAttribution {
    pub buyer: Pubkey,
    pub source: Option<Pubkey>,
    pub source_class: FundingSourceClass,
    pub selected_source_amount_lamports: u64,
    pub total_candidate_amount_lamports: u64,
    pub attribution_confidence: f64,
    pub reason: FundingAttributionReason,
}

pub enum FundingSourceClass {
    NonNeutral,
    Neutral,
    Unknown,
    LowConfidence,
    Unorderable,
}

pub enum FundingAttributionReason {
    SelectedDominantSource,
    NoCandidateTransfer,
    DustFilteredOnly,
    NoPreBuyTransfer,
    SameSlotOrderingUnavailable,
    NeutralDominantSource,
    LowAttributionConfidence,
    IndexCold,
    LaneUnavailable,
    GapSuspected,
}
```

## 6. Ordering and Anti-Leakage

Strict rule: transfer fundingowy musi poprzedzac buy, ktory wyjasnia.

```text
transfer.slot < buy.slot
  => OK

transfer.slot > buy.slot
  => reject as PostBuy

transfer.slot == buy.slot
  => OK only if:
     transfer.tx_index < buy.tx_index
     albo ta sama signature i instruction_index/event_ordinal dowodzi kolejnosci

transfer.slot == buy.slot and no comparable tx_index/order
  => SameSlotOrderingUnavailable
```

Do modelu domenowego trzeba dodac addytywne `tx_index` do trade/buy events i funding transfer events. Arrival/order timestamps moga byc evidence availability, ale nie moga byc proof of chain order dla FSC v2 decision-time.

Liczniki evidence:

```rust
pub same_slot_unorderable_count: u16,
pub post_buy_filtered_count: u16,
pub same_signature_ordered_count: u16,
pub tx_index_ordered_count: u16,
```

## 7. HHI and Score Semantics

Primary metric:

```rust
pub fsc_v2_hhi_norm_count: Option<f64>;
```

Formula:

```text
m = known_non_neutral_funded_buyers
p[source] = count(source) / m
hhi = sum(p[source]^2)
fsc_v2_hhi_norm_count = (hhi - 1/m) / (1 - 1/m)
```

Rules:

- `m < 2 => None / InsufficientNonNeutralSupport`.
- Clamp to `0..1`.
- `UNKNOWN != 0.0`.
- `NEUTRAL != cabal`.

Control examples:

- `[5] => 1.0`
- `[1,1,1,1,1] => 0.0`
- `[4,1] => 0.60`

Diagnostics:

```rust
pub fsc_v2_top1_share_count: Option<f64>;
pub fsc_v2_top1_share_sol: Option<f64>;
pub fsc_v2_hhi_norm_sol_weighted_excess: Option<f64>;
```

Weighted excess:

```text
w_b = first_buy_sol_amount_b / total_first_buy_sol_amount
buyer_weight_hhi = sum(w_b^2)
source_weight[source] = sum(w_b for buyers attributed to source)
source_hhi = sum(source_weight[source]^2)
fsc_v2_hhi_norm_sol_weighted_excess = (source_hhi - buyer_weight_hhi) / (1 - buyer_weight_hhi)
```

Primary score remains count-based normalized HHI. Weighted values are diagnostics until offline validation proves utility.

## 8. Neutral, Unknown and Dust Semantics

Neutral funders:

- excluded from scoring HHI;
- included in raw diagnostics;
- can drive `Degraded` / `NeutralOnly`;
- must not produce fake-clean `0.0`.

Required fields:

```rust
pub raw_hhi_including_neutral: Option<f64>;
pub scoring_hhi_non_neutral: Option<f64>;
pub neutral_count: u8;
pub neutral_share: f64;
pub known_non_neutral_buyers: u8;
pub non_neutral_known_coverage: f64;
pub neutral_funder_set_version: Option<String>;
pub neutral_funder_set_hash: Option<String>;
```

Dust policy:

```text
storage-level:
  fsc_min_abs_store_lamports = 1_000_000

attribution-level:
  fsc_min_abs_attribution_lamports = 10_000_000
  fsc_min_rel_to_buy = 0.20
```

Primary attribution starts conservative:

```text
amount >= fsc_min_abs_attribution_lamports
AND
amount >= fsc_min_rel_to_buy * first_buy_amount_lamports
```

Threshold variants `5_000_000`, `10_000_000`, `50_000_000` lamports are computed offline from raw funding events.

## 9. Decision-Time vs Eventual FSC

Two snapshot modes are mandatory:

```rust
pub enum FscSnapshotMode {
    DecisionTime,
    EventualPostfill,
}
```

- `DecisionTime` uses only funding events available to runtime at the decision snapshot.
- `EventualPostfill` may use later-arriving events in offline/replay diagnostics.
- Future Gatekeeper can only use `DecisionTime`.

Evidence row must include:

```rust
pub snapshot_mode: FscSnapshotMode;
pub feature_cutoff_ts_ms: u64;
pub feature_cutoff_slot: Option<u64>;
pub computed_at_recv_ts_ns: u128;
```

## 10. Lane Health Contract

Clean FSC requires:

- `index_warm = true`;
- `stream_connected = true`;
- `gap_suspected = false`;
- lane lag below configured threshold;
- no reconnect gap inside active lookback window.

Proposed health struct:

```rust
pub struct FundingLaneHealth {
    pub index_warm: bool,
    pub stream_connected: bool,
    pub last_event_age_ms: u64,
    pub watermark_slot: Option<u64>,
    pub lag_slots_vs_trade_lane: Option<i64>,
    pub reconnect_epoch: u64,
    pub gap_suspected: bool,
    pub queue_depth: usize,
    pub dropped_events: u64,
}
```

Because NLN Subscribe is live-only in the known public contract and has no offset/cursor resume, reconnect without resume must degrade FSC until warmup/lookback coverage is rebuilt.

## 11. FscV2Evidence Payload

Minimum additive payload:

```rust
pub struct FscV2Evidence {
    pub version: FscVersion,
    pub attribution_scope: FscAttributionScope,
    pub snapshot_mode: FscSnapshotMode,

    pub total_buyers: u8,
    pub known_buyers: u8,
    pub known_non_neutral_buyers: u8,
    pub unknown_count: u8,
    pub neutral_count: u8,
    pub low_confidence_count: u8,
    pub same_slot_unorderable_count: u16,

    pub known_coverage: f64,
    pub non_neutral_known_coverage: f64,
    pub neutral_share: f64,

    pub top1_share_count: Option<f64>,
    pub top1_share_sol: Option<f64>,
    pub hhi_norm_count: Option<f64>,
    pub hhi_norm_sol_weighted_excess: Option<f64>,
    pub raw_hhi_including_neutral: Option<f64>,
    pub scoring_hhi_non_neutral: Option<f64>,

    pub top_funder: Option<FundingSourceKey>,
    pub top_funder_count: u8,
    pub top_funder_buy_sol: f64,
    pub source_counts: Vec<(FundingSourceKey, u8)>,

    pub attribution_confidence_mean: Option<f64>,
    pub attribution_confidence_min: Option<f64>,

    pub dust_filtered_count: u16,
    pub post_buy_filtered_count: u16,
    pub rel_too_small_count: u16,

    pub index_warm: bool,
    pub capture_ready: bool,
    pub status: FscEvidenceStatus,
    pub excluded_reason: Option<FscExcludedReason>,

    pub funding_lane_watermark_slot: Option<u64>,
    pub max_buy_slot: Option<u64>,
    pub funding_lane_lag_slots: Option<i64>,
    pub stream_epoch: u64,
    pub gap_suspected: bool,

    pub min_abs_store_lamports: u64,
    pub min_abs_attribution_lamports: u64,
    pub min_rel_to_buy: f64,
    pub ttl_seconds: u64,

    pub neutral_funder_set_version: Option<String>,
    pub neutral_funder_set_hash: Option<String>,
    pub config_hash: String,
    pub provider: String,
    pub source_topics: Vec<String>,
}
```

`MetricValue.value` should map to `hhi_norm_count`, not top1 share or weighted diagnostics.

## 12. Implementation Phases

### PR-FSC0 - ADR Amendment and Scope Contract

Goal: formally change the scope from "FSC de-scoped due single-stream constraint" to "FSC v2 capture/evidence allowed through NLN Program Streams, decision off".

In scope:

- add ADR amendment to `ADR-0130`;
- declare NLN Program Streams as semantic FSC capture lane candidate;
- keep active Gatekeeper off;
- keep R2 SSOT outside Program Streams;
- define single-hop native-SOL scope;
- define decision-time vs eventual split;
- prohibit legacy field semantic overwrite.

Acceptance:

- plan and ADR both say capture ON, decision OFF;
- no config or runtime activation;
- no full-chain relabeling for filtered streams.

### PR-FSC1 - Config and Rollout Profile

Goal: add inert `fsc_v2` and `seer.program_streams` config surfaces.

Acceptance:

- defaults are disabled/inert;
- all new fields use backward-compatible serde defaults;
- existing V3 primary-only profiles remain decision-neutral;
- new capture-only profile enables only evidence export.

### PR-FSC2 - NLN Program Streams Client

Goal: implement JSON-mode `ListTopics` and `Subscribe` client.

Acceptance:

- no RPC hot path;
- no NLN RPC coverage proof;
- decode errors, reconnects and stalls are metered;
- offset is diagnostic-only.

### PR-FSC3 - NLN Normalization to Ghost Events

Goal: normalize create/trade/transfers into existing Ghost boundaries.

Acceptance:

- create has provenance;
- trade maps `user` to buyer;
- transfers are native SOL only for primary FSC;
- `tx_index` is additive and backward compatible;
- `full_chain_coverage=true` only for healthy dedicated transfer lane.

### PR-FSC4 - Bounded Funding Index v2

Goal: bounded rolling transfer index with TTL, caps, dedupe and lane health.

Acceptance:

- funding lane cannot block trade/Gatekeeper/executor;
- overload degrades FSC only;
- no unbounded queues or maps.

### PR-FSC5 - Funding Attribution v2

Goal: dominant meaningful source attribution.

Acceptance:

- first buy per buyer is order-key based;
- latest dust transfer cannot poison source;
- same-slot missing order becomes unorderable;
- neutral/unknown/low-confidence are explicit states.

### PR-FSC6 - Metrics and Evidence Payload

Goal: compute FSC v2 metrics export-only.

Acceptance:

- sample-normalized HHI tests pass;
- weighted diagnostics are separate;
- unknown and neutral do not become fake-clean;
- `FscV2Evidence` serializes as additive snapshot evidence.

### PR-FSC7 - Materialization, Logger and Gatekeeper Guard

Goal: materialize FSC v2 without policy impact.

Acceptance:

- `MaterializedFeatureSet.sybil_resistance.funding_source_v2` is additive;
- legacy field semantics are preserved;
- V2/V3 policy ignores FSC v2 when `decision_enabled=false`;
- DecisionLogger gets additive schema fields.

### PR-FSC8 - Dataset and Provider Qualification

Goal: qualify NLN and create selector artifacts.

Artifacts:

- `logs/nln/<scope>/pumpfun_create_raw_v1.jsonl`;
- `logs/nln/<scope>/pumpfun_trade_raw_v1.jsonl`;
- `logs/nln/<scope>/system_transfers_raw_v1.jsonl`;
- `datasets/selector/<scope>/nln_candidate_birth_v1.jsonl`;
- `datasets/selector/<scope>/funding_events_v1.jsonl`;
- `datasets/selector/<scope>/fsc_snapshots_v2.jsonl`;
- `reports/selector/<scope>/fsc_coverage_v2.json`;
- `reports/selector/<scope>/nln_provider_benchmark_v1.json`;
- `reports/selector/<scope>/decision_time_vs_eventual_fsc_v1.json`.

Acceptance:

- minimum 24h, preferred 72h benchmark;
- compare to Chainstack/raw Yellowstone/archive-capable audit source;
- no FSC scoring without denominator and resolved R1/R2 labels.

### PR-FSC9 - Shadow Policy Counterfactual

Goal: show what FSC would have done, without doing it.

Acceptance:

- no active verdict drift;
- shadow-only fields show possible soft points/reasons;
- counterfactual impact is reportable offline.

## 13. Future Active Gatekeeper Conditions

Do not set `fsc_v2.decision_enabled=true` until all are true:

1. NLN funding lane stable for 24-72h.
2. `system.transfers` coverage verified against audit/raw source.
3. `pumpfun.trade` coverage verified.
4. Decision-time FSC separated from eventual FSC.
5. Leakage audit PASS.
6. Neutral funder set versioned and hashed.
7. Unknown rate, neutral share and known non-neutral coverage measured on real launches.
8. `baseline_core + FSC` improves holdout R1/R2 outcomes.
9. No R1/lifecycle regression.
10. No material organic false reject cluster.
11. No backpressure/latency impact on trade lane, Gatekeeper or executor.
12. R2 canonical market path remains separate SSOT.
13. Rollback is a config-only flip: `fsc_v2.decision_enabled=false`.

First possible activation shape:

```toml
[fsc_v2]
capture_enabled = true
feature_emit_enabled = true
decision_enabled = true
hard_reject_enabled = false
mode = "soft_feature"
```

Hard reject remains out of scope and requires a separate ADR.

## 14. Test Plan

Rust:

- `cargo fmt --all --check`
- `cargo check -p seer -p ghost-launcher -p ghost-brain -p ghost-core`
- `cargo test -p seer --lib nln_program_streams`
- `cargo test -p seer --test source_router`
- `cargo test -p ghost-launcher --lib tx_intelligence::funding_source::tests`
- `cargo test -p ghost-launcher --test gatekeeper_policy_tests`
- `cargo test -p ghost-launcher --test gatekeeper_v3_tests`

Python:

- `python3 -m unittest scripts/test_selector_pipeline.py`
- add selector tests for FSC cutoff, `user` alias, quote provenance, decision-time vs eventual and no-outcome-leakage.

Required scenarios:

- missing topic in `ListTopics` fails closed;
- JSON numeric strings decode correctly;
- duplicate transfer/trade does not inflate FSC;
- first buy per buyer is selected by order key;
- latest dust transfer cannot overwrite dominant source;
- same-slot cross-signature without `tx_index` is unorderable;
- neutral-only is degraded, not `0.0`;
- unknown-only is unavailable/degraded, not `0.0`;
- HHI `[5] => 1.0`, `[1,1,1,1,1] => 0.0`, `[4,1] => 0.60`;
- weighted excess subtracts buyer amount inequality baseline;
- V3 hard-risk ignores FSC when `decision_enabled=false`.

## 15. Explicit Non-Goals

- No active FSC hard reject.
- No active FSC penalty in current V3 primary-only profiles.
- No silent overwrite of legacy `funding_source_concentration`.
- No UNKNOWN or neutral-only fake-clean.
- No latest inbound attribution.
- No arrival-order proof for same-slot cross-signature.
- No eventual FSC as live decision FSC.
- No WSOL/SPL in primary FSC.
- No NLN RPC as primary audit/replay backend.
- No Program Streams as R2 SSOT.
- No R2 NO-GO removal because FSC gained a funding lane.

## 16. Delegation Trace

```yaml
delegation_trace:
  task_classification: "cross-cutting FSC v2 capture/evidence architecture"
  routing_performed: true
  primary_specialist: "Seer Ingest Event Integrity Specialist"
  supporting_specialists_considered:
    - "SSOT Feature Materialization Guardian"
    - "Gatekeeper Policy Auditor"
    - "Decision Logging Replay Analyst"
    - "Config Rollout Safety Reviewer"
    - "Solana Execution Path Engineer"
  specialist_docs_loaded:
    - "AGENTS.md repository orchestration"
    - "newproviderr.md provider specification"
    - "ghost-execution skill"
    - "trading-systems skill"
    - "statistical-research-engine skill"
    - "solana-pumpfun-architect skill"
  skills_used:
    - "ghost-execution"
    - "trading-systems"
    - "statistical-research-engine"
    - "solana-pumpfun-architect"
  fast_path_used: false
  contracts_checked:
    - "MaterializedFeatureSet remains canonical feature snapshot"
    - "FSC v2 is additive export-only until explicit decision gate"
    - "Legacy funding_source_concentration semantics are not silently changed"
    - "Unknown/neutral funding does not become fake-clean"
    - "Program Streams are not promoted to R2 SSOT"
    - "Decision-time evidence is separated from eventual postfill"
    - "Shadow/live boundary remains intact"
  unresolved_routing_uncertainty: []
```
