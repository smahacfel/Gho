# Ghost Repository Agent Orchestration

This repository contains Ghost: a low-latency, event-driven, selective pump.fun sniper on Solana.

Ghost is not HFT.
Ghost is not MEV.
Ghost is not a generic prediction engine.
Ghost is a selective decision runtime that rejects obvious traps and only enters when decision-time evidence survives the observation window.

The primary goals are:

- preserve SSOT integrity
- preserve deterministic Gatekeeper decisions
- preserve replayability and auditability
- preserve typed verdicts and reason codes
- preserve shadow/live separation
- avoid accidental legacy-path revival
- avoid broad rewrites for localized tasks

Use this file as the always-on repository orchestration layer.

Detailed specialist role definitions live in: docs/agents/

Project skills live in: .codex/skills/

Load detailed agent docs or skill references only when needed.

---

# 1. Core Ghost Runtime Truth

Assume the active Ghost architecture follows this high-level path unless current code/config proves otherwise:

→ Yellowstone / Seer
→ Event Bus
→ OracleRuntime
→ PoolObservationSession
→ AccountStateCore / TxIntelligence / Checkpoints / GatekeeperBuffer
→ PoolObservationSession::materialize_features()
→ MaterializedFeatureSet
→ Gatekeeper V2/V2.5 policy evaluation
→ IWIM veto if BUY path requires it
→ shadow execution / simulation
→ post-buy lifecycle
→ DecisionLogger / JSONL
→ replay / audit evidence


Treat `MaterializedFeatureSet` as the canonical decision snapshot.

Treat `PoolObservationSession::materialize_features()` as the main boundary where runtime/component state becomes immutable decision evidence.

Treat current execution as shadow-only unless active config/code explicitly proves live behavior is enabled.

Do not revive deprecated or legacy decision paths unless the task explicitly asks for legacy analysis.

Dangerous legacy concepts include:

* HyperPrediction / Chaos as active Gatekeeper dependencies
* deprecated `score_pool()`-style decision flow
* legacy `PoolScored` production behavior if marked no-op/deprecated
* stale schema/config assumptions from old docs

---

# 2. Non-Negotiable Repository Rules

These rules override local convenience.

1. `MaterializedFeatureSet` is the canonical Gatekeeper decision snapshot.

2. Gatekeeper policy must not recompute authoritative features from competing mutable sources.

3. No live-state reads during policy evaluation unless they are explicitly part of the materialization contract.

4. Every terminal decision must have a typed verdict and reason code.

5. Generic `REJECT` or generic failure buckets are forbidden when a more specific class exists.

6. Hard safety filters must not be bypassed by soft scores.

7. Shadow simulation is not live inclusion.

8. Submit is not confirmation.

9. Unknown execution status is not success.

10. Decision logs must preserve enough evidence to audit and replay decisions.

11. New config fields must preserve backward compatibility and use `#[serde(default)]` where old configs must still load.

12. Thresholds that affect decisions must be config-driven unless explicitly constant by design.

13. Do not silently change active behavior while claiming the change is diagnostic-only.

14. Do not blur shadow/live behavior.

15. Do not widen synchronization scope or introduce hot-path blocking without explicit justification.

16. Do not make broad architecture rewrites for localized tasks.

---

# 3. Default Intake Procedure

For every non-trivial task:

1. Identify the active runtime area touched.
2. Identify whether the path is active, shadow-only, test-only, or legacy.
3. Identify contracts at risk:

   * SSOT / `MaterializedFeatureSet`
   * session lifecycle
   * Gatekeeper policy order
   * verdict/reason codes
   * config compatibility
   * shadow/live boundary
   * DecisionLogger / replay
   * Solana execution validity
   * ingest identity/order semantics
4. Select the correct specialist role.
5. Load the relevant skill.
6. Read the specialist doc only if the task is non-local, risky, ambiguous, or touches that specialist’s contract.
7. Make the minimal safe change.
8. Re-check invariants before finalizing.

For narrow localized tasks, use the fast path.

---

# 4. Specialist Role Routing

The following specialist roles are logical orchestration roles. They are not separate native Codex agents. Use them to choose the correct reasoning mode, skill, and reference document.

Full definitions are stored in: docs/agents/


## 4.1 Ghost Runtime Coordinator

Use when the task is broad, cross-cutting, ambiguous, or touches multiple Ghost components.

Read: docs/agents/ghost-runtime-coordinator.md

Use for:

* multi-component Ghost changes
* active-vs-legacy classification
* deciding which specialist should handle the task
* reviewing changes that touch several runtime areas
* protecting Ghost-wide contracts

Primary skills:

* `ghost-execution`
* `trading-systems`
* `abstract-reasoning`

Key question: What active Ghost runtime path does this task touch, which contracts are at risk, and which specialist should handle it?

---

## 4.2 SSOT Feature Materialization Guardian

Use when the task touches feature ownership, materialization, or decision snapshots.

Read: docs/agents/ssot-feature-materialization-guardian.md

Use for:

* `MaterializedFeatureSet`
* `PoolObservationSession::materialize_features()`
* feature ownership
* AccountStateCore / TxIntelligence / Checkpoint / Sybil / Alpha boundaries
* fallback/degraded feature behavior
* replay-safe materialization
* preventing dual-authority feature computation

Primary skills:

* `ghost-execution`
* `rust-master`
* `trading-systems`

Key question:

Is the decision feature model still single-source-of-truth, materialized exactly once at the correct boundary, immutable during evaluation, and replayable?

---

## 4.3 Gatekeeper Policy Auditor

Use when the task touches Gatekeeper decision behavior.

Read: docs/agents/gatekeeper-policy-auditor.md

Use for:

* `gatekeeper_policy.rs`
* Gatekeeper V2 / V2.5 policy
* hard fails
* core phases
* PDD / DOW / TAS / APS behavior
* Alpha / Prosperity / Sybil / Curve gates
* verdict taxonomy
* reason chains
* false BUY / false REJECT / TIMEOUT analysis

Primary skills:

* `ghost-execution`
* `trading-systems`
* `statistical-research-engine`

Key question: Given this materialized snapshot and config, why did Gatekeeper produce this verdict, and is that policy behavior correct?

---

## 4.4 Oracle Session Runtime Engineer

Use when the task touches runtime orchestration, sessions, event routing, deadlines, or per-pool tasks.

Read: docs/agents/oracle-session-runtime-engineer.md

Use for:

* `OracleRuntime`
* `pool_observation_task`
* session lifecycle
* event routing
* observation deadlines
* orphan transactions
* `tokio::select!` behavior
* terminal verdict application
* cleanup after verdict
* runtime race conditions

Primary skills:

* `ghost-execution`
* `rust-master`
* `trading-systems`

Key question: Did the runtime deliver the right events to the right session, within the right time window, without races, duplication, stale timing, or lifecycle corruption?

---

## 4.5 Seer Ingest Event Integrity Specialist

Use when the task touches ingestion, Yellowstone/Geyser, parsers, event identity, or event ordering.

Read: docs/agents/seer-ingest-event-integrity-specialist.md

Use for:

* Seer
* Yellowstone / Geyser gRPC
* binary/curve parsers
* event normalization
* `GhostEvent::NewPoolDetected`
* `GhostEvent::PoolTransaction`
* `GhostEvent::AccountUpdate`
* `GhostEvent::FundingTransferObserved`
* deduplication
* timestamp/slot semantics
* stream lag/reconnect behavior
* parser confidence/degraded behavior

Primary skills:

* `ghost-execution`
* `solana-pumpfun-architect`
* `rust-master`

Key question: Did Ghost ingest, parse, normalize, identify, order, and route chain events correctly enough for downstream runtime and decision logic to trust them?

---

## 4.6 Solana Execution Path Engineer

Use when the task touches transaction construction, simulation, live sender, blockhash, fees, retries, confirmation, or reconciliation.

Read: docs/agents/solana-execution-path-engineer.md

Use for:

* DirectBuyBuilder / DirectSellBuilder
* TriggerComponent execution behavior
* shadow simulation
* Helius Sender / LiveTxSender
* blockhash lifecycle
* compute budget / priority fees
* transaction retry
* simulation vs inclusion
* confirmation tracking
* account contention
* slippage / changed-liquidity invalidation
* post-Gatekeeper execution handoff

Primary skills:

* `solana-pumpfun-architect`
* `ghost-execution`
* `rust-master`
* `trading-systems`

Key question: Given an approved Ghost intent, can the system build, submit, observe, confirm, and reconcile a valid Solana transaction attempt without stale state, duplicate execution, or hidden execution drift?

---

## 4.7 Decision Logging Replay Analyst

Use when the task touches DecisionLogger, JSONL, schema, replay, shadow lifecycle proof, or audit evidence.

Read: docs/agents/decision-logging-replay-analyst.md

Use for:

* `DecisionLogger`
* JSONL schema
* Gatekeeper decision records
* verdict/reason-code logging
* PDD / DOW / TAS / APS / IWIM diagnostics in logs
* shadow lifecycle proof
* replay reconstruction
* schema evolution
* false BUY / false REJECT investigation from logs
* auditability of decision evidence

Primary skills:

* `ghost-execution`
* `trading-systems`
* `large-data-analytics`
* `statistical-research-engine`

Key question: Can this decision be reconstructed, explained, replayed, compared, and audited later from durable evidence without relying on hidden runtime state?


---

## 4.8 Config Rollout Safety Reviewer

Use when the task touches config, thresholds, defaults, serde compatibility, rollout, modes, or shadow/live behavior.

Read: docs/agents/config-rollout-safety-reviewer.md

Use for:

* `ghost_brain_config.toml`
* config structs
* Gatekeeper V2 / V2.5 config
* PDD / DOW / TAS / APS config
* Alpha / Prosperity / Sybil / Curve thresholds
* IWIM config
* execution mode / entry mode
* `#[serde(default)]`
* config migrations
* rollout safety
* threshold regression risk

Primary skills:

* `ghost-execution`
* `trading-systems`
* `rust-master`

Key question: Can this configuration change be loaded safely, understood later, rolled back if needed, and applied without silently weakening Ghost’s decision or execution safety?


---

# 5. Skill Routing

Use project skills from: .codex/skills/
 
Each skill has: 
* SKILL.md
* references.md

Load `SKILL.md` when the task matches the skill description.

Load `references.md` only when deeper guidance is needed.

## Primary skill map

Use `ghost-execution` for:

* Ghost-specific architecture
* Gatekeeper flow
* SSOT contracts
* decision logging
* shadow/live boundary
* current runtime path
* active-vs-legacy separation

Use `rust-master` for:

* Rust ownership
* Tokio/async
* hot-path performance
* bounded concurrency
* replay-safe runtime code
* error handling
* unsafe review

Use `solana-pumpfun-architect` for:

* Solana transaction lifecycle
* pump.fun / PumpSwap account semantics
* Yellowstone/Geyser execution implications
* blockhash/fees/account contention
* simulation vs inclusion

Use `trading-systems` for:

* decision system architecture
* risk/exposure
* execution orchestration
* reconciliation
* replay/recovery
* hard vs soft constraints

Use `statistical-research-engine` for:

* signal validation
* separability testing
* calibration
* leakage checks
* robustness / walk-forward
* threshold validation

Use `large-data-analytics` for:

* event-stream discovery
* feature mining
* pattern/anomaly discovery
* bounded-window analytics
* runtime-feasible feature candidates

Use `abstract-reasoning` for:

* ambiguous architecture decisions
* contradiction analysis
* cross-domain trade-offs
* deciding handoff boundaries

---

# 6. References Loading Rules

Do not load all references by default.

Read specialist docs or `references.md` only when:

* task is ambiguous
* task is cross-cutting
* task touches SSOT or Gatekeeper policy
* task changes execution/live/shadow behavior
* task changes config defaults or thresholds
* task changes JSONL schema or replay evidence
* task involves Solana execution validity
* task changes ingestion identity/order semantics
* task requires deep reasoning or trade-off analysis

Do not read references for:

* comments
* formatting
* tiny localized helpers
* obvious single-file fixes
* tests unrelated to runtime contracts
* spelling or naming-only changes

If references are loaded, state briefly which reference was used and why.

---

# 7. Fast Path Rule

For localized, low-risk edits:

* do not run full architecture review
* do not read all specialist docs
* do not broaden the task
* preserve relevant invariants
* make the smallest safe change
* mention that no broader Ghost contract impact was detected

Fast path applies when the task does not affect:

* `MaterializedFeatureSet`
* feature materialization
* Gatekeeper policy
* verdicts / reason codes
* session lifecycle
* event routing
* ingestion parser semantics
* Solana execution
* config thresholds/defaults
* DecisionLogger / replay
* shadow/live boundary
* active-vs-legacy separation

---

# 8. Active-vs-Legacy Discipline

Before modifying decision/runtime behavior, classify the path:

* active runtime
* shadow-only
* diagnostic-only
* test-only
* legacy/deprecated

Never promote legacy/test helpers into active behavior accidentally.

Never revive HyperPrediction/Chaos/old scoring flow into active Gatekeeper without explicit architectural approval.

Do not trust old docs over current code/config.

If documentation and code conflict, inspect current code/config and state uncertainty.

---

# 9. Output Expectations

For non-trivial tasks, include:

```yaml
task_classification: string
primary_specialist: string
supporting_specialists: list
skills_used: list
references_loaded: list
runtime_area_touched: list
contracts_at_risk: list
active_or_legacy_path: string
recommended_action: string
verification_steps: list
risk_level: low | medium | high
```

For implementation tasks, also include:

```yaml
files_touched: list
invariants_preserved: list
tests_or_checks: list
handoffs_needed: list
```

For simple fast-path tasks, a shorter plain response is acceptable.

---

# 10. Repository-Level Failure Modes

Stop and correct course if any of these appear:

* `MaterializedFeatureSet` bypass
* duplicate feature authority
* live-state read during Gatekeeper policy evaluation
* terminal verdict without reason code
* generic rejection replacing typed verdict
* config threshold hardcoded in policy path
* config field added without backward-compatible default
* DecisionLogger schema changed destructively
* shadow simulation treated as live inclusion
* live execution enabled implicitly
* late event rewriting historical verdict
* duplicate event counted as unique
* stale account update overwriting canonical state
* ShadowLedger silently promoted to canonical runtime truth
* HyperPrediction/Chaos revived into active Gatekeeper path
* unbounded retry loop
* submit treated as confirmation
* unknown execution status treated as success
* lock held across `.await`
* broad refactor introduced for localized task

---

# 11. Verification Bias

Prefer verification over speculation.

When unsure:

* inspect current code
* inspect current config
* inspect README/audit only as supporting context
* state uncertainty explicitly
* ask for clarification only if a critical fact is missing
* otherwise proceed with bounded assumptions

Do not invent missing facts.

Do not present stale assumptions as current truth.

---

# 12. Delegation Trace Requirement

For every non-trivial task, the agent must leave an explicit delegation trace in the final response.

The goal is not to force specialist usage.
The goal is to make routing decisions auditable.

The agent must report:
- whether routing was performed,
- which primary specialist was selected,
- which supporting specialists were considered,
- which specialist documents were loaded,
- which specialist documents were intentionally not loaded and why,
- whether fast path was used,
- which repository contracts were checked.

Specialist documents should only be loaded when the task is non-local, risky, ambiguous, or touches that specialist’s contract.
Do not load specialist documents mechanically for trivial or clearly localized tasks.
For fast-path tasks, the agent must still state why no specialist delegation was needed.

## Required Delegation Trace Format

For non-trivial tasks, include:

delegation_trace:
  task_classification: string
  routing_performed: true
  primary_specialist: string
  supporting_specialists_considered: list
  specialist_docs_loaded: list
  specialist_docs_not_loaded:
    - name: string
    - reason: string
  skills_used: list
  fast_path_used: true | false
  contracts_checked: list
  unresolved_routing_uncertainty: list

For fast-path tasks, include:

delegation_trace:
  task_classification: "localized"
  routing_performed: true
  primary_specialist: "none"
  fast_path_used: true
  reason: string
  contracts_checked: list
  
---

# 13. Final Principle

This repository optimizes for selective decision integrity.

Protect:

* source of truth
* observation window
* deterministic policy
* typed verdicts
* reason-code auditability
* shadow/live clarity
* replay/reconstruction
* conservative behavior under uncertainty

Reject fragile reasoning.
Avoid unnecessary rewrites.
Use specialists deliberately.
Keep Ghost’s runtime contracts intact.