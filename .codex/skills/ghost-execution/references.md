## `ghost-execution/references.md`

# Ghost Execution Reference

This file expands the `ghost-execution` skill. Read it only when deeper Ghost-specific architecture, Gatekeeper policy, SSOT contracts, decision logging, module boundaries, replay safety, or component behavior is needed.

Use this reference for:

* Gatekeeper V2 / V2.5 changes
* policy ordering decisions
* PDD / DOW / TAS / APS analysis
* `MaterializedFeatureSet` / SSOT work
* observation-session lifecycle changes
* DecisionLogger / JSONL schema changes
* Ghost config changes
* shadow/live execution boundary analysis
* replay and audit requirements
* diagnosing BUY / REJECT / TIMEOUT behavior

Do not load this file for small localized Rust/Solana/statistical/data-mining tasks unless Ghost pipeline contracts are involved.

---

# 1. Current Project Identity

Ghost is a selective, automated pump.fun trading system on Solana.

It is:

* event-driven
* Rust-based
* low-latency
* bounded observation-window based
* decision-audited
* shadow-only unless explicitly changed
* conservative under uncertainty

It is not:

* HFT market making
* MEV searcher
* generic Solana bot
* generic ML prediction engine
* discretionary trading tool

Core principle:

```text
Ghost rejects obvious traps and only enters when evidence survives the observation window.
````

Ghost should not be reframed as a system that predicts every future winner.

Its edge comes from selective filtering, not broad activity.

---

# 2. Active Runtime Model

The active runtime should be treated as:

```text
Seer / Yellowstone ingestion
→ Event Bus
→ OracleRuntime
→ PoolObservationSession
→ AccountStateCore / TxIntelligence / Checkpoints / GatekeeperBuffer
→ PoolObservationSession::materialize_features()
→ MaterializedFeatureSet
→ Gatekeeper V2/V2.5 policy
→ optional IWIM veto after BUY path
→ shadow execution / simulation
→ post-buy runtime
→ DecisionLogger / JSONL
→ replay / audit evidence
```

Do not assume old architectural diagrams are current unless verified against code.

Before changing execution behavior, verify:

* active config
* active mode
* active feature flags
* active execution mode
* decision logger schema constants
* whether path is production, shadow, test, or legacy

---

# 3. Active vs Legacy Boundary

Treat these boundaries carefully.

## Active / current concepts

* `OracleRuntime`
* `PoolObservationSession`
* `MaterializedFeatureSet`
* `AccountStateCore`
* `GatekeeperBuffer`
* `TxIntelligenceEngine`
* `CheckpointEngine`
* `CurveReadinessFeatures`
* `SybilResistanceFeatures`
* `AlphaFingerprintFeatures`
* Gatekeeper V2/V2.5 policy evaluation
* DOW / TAS / PDD / APS where enabled/configured
* IWIM veto after Gatekeeper BUY path where configured
* DecisionLogger / JSONL audit
* shadow-only execution path unless live is explicitly enabled

## Legacy / dangerous-to-revive concepts

* `HyperPredictionOracle` as active Gatekeeper dependency
* `ChaosEngine` as active production decision dependency
* deprecated `score_pool()` style paths
* generic `PoolScored` production flow if marked legacy/no-op
* old schema assumptions without checking constants
* standalone `mode = "v25"` assumptions if code/config uses a different active model

Rules:

* never revive legacy decision paths accidentally
* never couple HyperPrediction/Chaos into Gatekeeper without explicit V3-level design decision
* never treat deprecated/test-only helpers as production flow
* never trust historical docs over current code/config

---

# 4. SSOT: MaterializedFeatureSet

`MaterializedFeatureSet` is the decision snapshot.

It aggregates decision-relevant state such as:

* account features
* tx intelligence features
* checkpoint-derived features
* risk flags
* session metadata
* curve readiness
* sybil resistance
* alpha fingerprint
* related materialized sequence/trajectory fields where present

Canonical rule:

```text
session state and component-owned state
→ PoolObservationSession::materialize_features()
→ MaterializedFeatureSet
→ Gatekeeper assessment / policy
```

Important ownership principles:

* Account state comes from `AccountStateCore` where canonical.
* Tx features come from `TxIntelligenceEngine`.
* Checkpoint features come from checkpoint materialization.
* Curve dynamics may supplement checkpoint/account-derived fields through the session materialization boundary.
* Alpha fingerprint comes from early fingerprint aggregation.
* CPV/FSC/sybil fields are materialized into the snapshot.
* Gatekeeper policy consumes the snapshot; it should not independently reconstruct the world.

SSOT violations include:

* recomputing a feature inside policy from raw events when it should come from snapshot
* using `GatekeeperBuffer` as an alternate canonical authority outside materialization
* reading live account state during policy after snapshot materialization
* overwriting materialized fields downstream
* allowing two components to own the same semantic feature

When adding new features:

1. define owner
2. define materialization point
3. define degraded-input behavior
4. add optional/default-compatible fields where needed
5. ensure decision logging can capture/reconstruct it
6. preserve replay determinism

---

# 5. AccountStateCore vs ShadowLedger

Treat state authority carefully.

## AccountStateCore

Use as canonical runtime source for on-chain pool/account state when available.

Expected responsibilities:

* canonical pool/account features
* account update application
* state phase tracking
* update count / finality/freshness
* price/market-cap/bonding progress where available

## ShadowLedger

Treat as support, bootstrap, simulation, history, or forensic state unless current code explicitly assigns active authority.

Do not silently promote ShadowLedger to canonical runtime truth.

Dangerous mistakes:

* using ShadowLedger state to override canonical AccountStateCore without explicit policy
* mixing ShadowLedger and AccountStateCore values without finality/freshness labeling
* treating fallback state as equivalent to canonical state
* hiding fallback source from decision logs

Any fallback must be visible in diagnostics.

---

# 6. Observation Session Lifecycle

Each new pool should be handled through a bounded session.

Typical lifecycle:

```text
CREATED / OBSERVED
→ ACCUMULATING
→ EVALUATING
→ DECIDED(BUY / REJECT / TIMEOUT / PENDING)
→ CLOSED
```

Session responsibilities:

* track pool identity
* collect transactions
* deduplicate transaction keys
* refresh account features
* update tx intelligence
* maintain checkpoints
* materialize features
* apply terminal verdict
* record diagnostics

Rules:

* observation deadline must be explicit
* terminal verdict must be explicit
* duplicate events must not inflate features
* late events after terminal verdict must not rewrite decision
* timestamp domains must not be mixed
* session-owned timestamps must not be overwritten by mirrored buffer timestamps
* diagnostics must preserve enough context to debug verdicts

Failure modes:

* session remains open after terminal verdict
* deadline uses mixed event/wall-clock time
* duplicate tx counted twice
* account updates mutate state mid-evaluation
* late pool metadata rewrites earlier assumption
* feature snapshot not logged or reconstructable

---

# 7. Gatekeeper Evaluation Model

Gatekeeper is feature-driven.

Preferred current shape:

```text
MaterializedFeatureSet
→ build assessment
→ evaluate hard filters
→ evaluate configured gates/layers
→ evaluate curve readiness/gate if required
→ typed verdict
```

General ordering principles:

1. hard safety filters first
2. PDD live veto where enabled/configured
3. core phase logic
4. sybil / alpha / prosperity / trajectory / adaptive modules according to configured policy
5. curve readiness/gate where required
6. DOW/early/normal/extended timing logic where enabled
7. IWIM veto after Gatekeeper BUY path, not before core Gatekeeper decision

Do not rely on historical ordering if code/config differs.

Always verify actual active order in `gatekeeper_policy.rs`, related modules, and config.

Rules:

* no generic `REJECT`
* no hidden score-to-buy shortcut
* no untyped failure path
* no bypass of hard filters
* no policy evaluation from partially materialized state
* no disabled threshold masquerading as valid safety check
* no changing module role without explicit design decision

---

# 8. Gatekeeper V2 Core Phases

Gatekeeper V2-style core evaluation includes feature groups such as:

* quantity
* velocity
* signer diversity
* volume sanity
* dev behavior
* bonding curve dynamics

Use current config/code as source of truth for exact thresholds.

Do not hardcode values from docs unless task explicitly updates config.

Typical failure categories include:

* insufficient activity
* unhealthy timing
* poor diversity
* suspicious volume structure
* dev-related risk
* curve/market-cap/bonding issues
* extreme concentration
* failed/stale/slow pool behavior

Core phase rules:

* phase pass/fail should be diagnosable
* reason chain should be preserved
* diagnostics should be logged
* thresholds should come from config
* degraded data should be explicit

---

# 9. Gatekeeper V2.5 Modules

V2.5 concepts include:

* PDD — pump/dump detection and veto-like diagnostics where configured
* DOW — dynamic observation window / early-normal-extended logic
* TAS — trajectory-aware scoring/modulation or reject logic according to config
* APS — adaptive prosperity scoring / shadow suggestions / regime heuristics depending on config

Important rule:

Do not assume module behavior from historical docs. Verify active config and code.

## PDD

PDD should detect pump/dump and unsafe entry conditions.

Typical concerns:

* entry drift
* ramping
* spike behavior
* flash crash risk
* sequence-level pump/dump diagnostics
* price path instability

PDD failure modes:

* drift threshold effectively disabled
* hard veto unintentionally bypassed
* partial PDD data treated as clean
* PDD shadow result treated as live veto incorrectly
* price history insufficient but used as strong evidence

## DOW

DOW controls timing of decision opportunities.

Typical windows:

* early
* normal
* extended

Rules:

* early BUY must be stricter than normal
* extended decision must be conservative
* timeout remains valid terminal outcome
* timing logic must not mix timestamp domains
* DOW checkpoint/shadow behavior must be distinguished from terminal live policy

## TAS

TAS evaluates trajectory/momentum/shape.

Rules:

* insufficient sample size must degrade/exclude TAS rather than fabricate confidence
* trajectory score must not be treated as stronger than data supports
* TAS role must match config/policy: modulator, diagnostic, or hard reject only when explicit
* sequence segmentation must be deterministic

## APS

APS handles prosperity/adaptive scoring behavior.

Rules:

* adaptive vs shadow-suggestion behavior must be distinguished
* small-sample regime detection should degrade safely
* regime-local heuristic behavior must be visible in diagnostics
* APS suggestions must not silently mutate live policy

---

# 10. IWIM Veto Gate

IWIM is a separate post-Gatekeeper safeguard where configured.

General policy:

```text
Gatekeeper BUY candidate
→ IWIM analysis / timeout handling
→ pass or veto according to strength/policy
```

Rules:

* do not run IWIM before Gatekeeper core decision unless architecture is explicitly changed
* IWIM timeout/unknown behavior must follow configured matrix
* IWIM veto reason must be logged
* IWIM must not silently mutate Gatekeeper assessment
* developer history fetch failures must be classified

Failure modes:

* IWIM coupled into Gatekeeper feature scoring
* IWIM timeout treated as pass for borderline decisions when policy forbids it
* IWIM result omitted from decision log
* dev wallet unknown mishandled

---

# 11. Verdict and Reason-Code Discipline

Every terminal outcome must be typed and explainable.

Expected classes:

* BUY / EARLY_BUY where supported
* REJECT_HARD_FAIL
* REJECT_CORE_FAIL
* REJECT_PUMP_AND_DUMP
* REJECT_ALPHA_GATE / low alpha equivalent
* REJECT_PROSPERITY / low prosperity equivalent
* REJECT_SYBIL / sybil interference equivalent
* REJECT_IWIM_VETO
* PENDING_CURVE / curve wait equivalent where modeled
* TIMEOUT
* other current enum variants in code

Always verify exact enum names in code.

Rules:

* no generic rejection without reason
* no failure swallowed into timeout unless actually timeout
* reason chain must survive logging
* new verdict variants require logging/replay compatibility
* changing verdict taxonomy requires migration awareness

---

# 12. Config Contracts

Config changes are high risk.

Rules:

* new config fields use `#[serde(default)]`
* threshold changes must be explicit
* behavior changes must be documented
* defaults must be safe
* old config files must deserialize unless migration is explicit
* live/shadow behavior must be clearly separated
* config-driven behavior must not be hardcoded in policy

Before changing config:

1. find current struct
2. find current TOML path
3. find defaults
4. find policy usage
5. find decision logging diagnostics
6. check tests
7. preserve backward compatibility

Do not assume historical values are current.

---

# 13. DecisionLogger / JSONL Contracts

Decision logs are part of the system’s correctness boundary.

Rules:

* verify current schema/version constants in code before editing
* prefer additive fields
* do not remove or rename old fields without migration
* include diagnostics needed to reconstruct decisions
* preserve reason code
* preserve verdict type
* preserve timing metadata
* preserve IWIM/PDD/TAS/APS diagnostics where relevant
* preserve feature snapshot or enough materialized diagnostics to audit

Failure modes:

* schema version hardcoded from stale doc
* field removed breaking downstream analysis
* new decision path lacks logging
* shadow/live result ambiguous
* reason chain lost
* diagnostics too sparse for replay

---

# 14. Shadow Execution Boundary

Current work should assume shadow-only unless config/code explicitly says otherwise.

Shadow lane means:

* simulate or shadow-run execution
* collect decision/execution evidence
* record lifecycle proof
* evaluate policy without real capital risk

Rules:

* shadow BUY is not live inclusion
* shadow success is not live safety proof
* shadow outcome must be labeled
* shadow lifecycle should not be merged with live P&L semantics
* live sender paths require Solana execution review
* live mode changes need extra safety review

If task touches transaction construction, landing probability, blockhash, fees, or live submit path, hand off to `solana-pumpfun-architect`.

---

# 15. File Map Guidance

Common Ghost areas:

## Runtime

* `ghost-launcher/src/oracle_runtime.rs`
* `ghost-launcher/src/session/observation.rs`
* `ghost-launcher/src/session/*`
* `ghost-launcher/src/events.rs`

## Gatekeeper

* `ghost-launcher/src/components/gatekeeper.rs`
* `ghost-launcher/src/components/gatekeeper_policy.rs`
* `ghost-launcher/src/components/gatekeeper_pdd.rs`
* `ghost-launcher/src/components/gatekeeper_pdd_sequence.rs`
* `ghost-launcher/src/components/gatekeeper_dow_timer.rs`
* `ghost-launcher/src/components/gatekeeper_trajectory.rs`
* `ghost-launcher/src/components/gatekeeper_adaptive_prosperity.rs`
* `ghost-launcher/src/components/iwim_veto.rs`

## Config

* `ghost-brain/ghost_brain_config.toml`
* `ghost-brain/src/config/*`
* `ghost-brain/src/config/gatekeeper_v25_config.rs`

## State

* `ghost-core/src/account_state_core/*`
* `ghost-core/src/checkpoint/*`
* `ghost-core/src/shadow_ledger/*`
* `ghost-core/src/tx_intelligence/*`

## Ingestion

* `off-chain/components/seer/src/grpc_connection.rs`
* `off-chain/components/seer/src/binary_parser.rs`
* `off-chain/components/seer/src/curve_parser.rs`
* `ghost-launcher/src/components/seer.rs`
* `ghost-launcher/src/components/snapshot_listener.rs`

## Logging / Replay

* `ghost-brain/src/oracle/decision_logger.rs`
* WAL / JSONL-related modules
* shadow lifecycle proof modules

Always verify current paths with `Grep`/`Read`.

Do not trust file maps blindly if repo changed.

---

# 16. Ghost-Specific Failure Modes

Detect and name:

* `MaterializedFeatureSet` bypass
* duplicate feature authority
* feature recomputation in policy path
* live-state read during evaluation
* post-verdict mutation rewriting history
* terminal verdict without typed reason
* generic rejection replacing classified rejection
* PDD/TAS/DOW/APS behavior changed without config review
* TAS/trajectory computed on insufficient sample without degradation
* PDD clean assumed from partial diagnostics
* entry drift threshold effectively disabled
* extended/late decision not treated conservatively
* IWIM moved before intended stage
* HyperPrediction/Chaos revived into active Gatekeeper path
* legacy mode broken unintentionally
* config field missing `#[serde(default)]`
* JSONL schema changed destructively
* DecisionLogger missing new path
* shadow/live boundary blurred
* timestamp domains mixed
* duplicate event counted as unique
* AccountStateCore bypassed by fallback state
* ShadowLedger silently promoted to canonical
* tests broken by pipeline contract change

If detected:

* stop
* name failure mode
* preserve current contract
* correct or hand off

---

# 17. Code Review Checklist

Before finalizing non-trivial Ghost work:

* active runtime path verified
* active config verified
* production/shadow/test/legacy path identified
* SSOT / `MaterializedFeatureSet` preserved
* feature owner identified
* observation lifecycle preserved
* decision snapshot boundary preserved
* Gatekeeper policy order preserved
* hard filters preserved
* V2/V2.5 module behavior matches config
* IWIM stage preserved
* typed verdict and reason code preserved
* DecisionLogger updated if needed
* JSONL compatibility preserved
* new config fields use `#[serde(default)]`
* shadow/live boundary preserved
* AccountStateCore authority preserved
* ShadowLedger fallback role not silently expanded
* replay/audit evidence preserved
* tests considered or updated
* specialist handoff used where appropriate

---

# 18. Handoff Rules

Ghost-specific orchestration stays here.

Hand off domain details:

* Rust ownership/concurrency/performance → `rust-master`
* Solana transactions/execution/blockhash/fees → `solana-pumpfun-architect`
* signal validation/calibration → `statistical-research-engine`
* raw pool data mining / feature discovery → `large-data-analytics`
* system-level trading/risk/reconciliation → `trading-systems`
* ambiguous architecture trade-offs → `abstract-reasoning`

If task spans multiple domains:

1. use `ghost-execution` to protect Ghost contracts
2. hand off specialist subproblem
3. return to Ghost contracts for integration

Do not let specialist skills violate Ghost SSOT or pipeline contracts.

---

# 19. Output Expectations

For Ghost-specific code/design output, include:

* touched component/stage
* active vs legacy path classification
* SSOT impact
* config impact
* verdict/reason-code impact
* logging/replay impact
* shadow/live impact
* failure modes considered
* tests or verification steps

When writing code:

* no placeholder logic in policy-critical paths
* no hardcoded thresholds unless clearly constant by design
* use structured tracing, not `println!`
* preserve config-driven behavior
* preserve typed verdicts
* preserve existing tests where possible
* keep new behavior isolated when feasible

---

# 20. Final Principle

Ghost’s core value is selective decision integrity.

A change is good only if it preserves:

* correct source of truth
* bounded observation
* deterministic decision logic
* typed verdicts
* reason-code auditability
* shadow/live clarity
* replay/reconstruction ability
* conservative behavior under uncertainty

Do not predict the future.
Eliminate obvious traps.
Do not bypass SSOT.
Do not revive legacy paths.
Do not confuse shadow evidence with live truth.