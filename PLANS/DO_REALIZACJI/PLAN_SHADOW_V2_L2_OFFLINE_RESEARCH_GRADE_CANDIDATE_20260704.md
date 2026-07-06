# Plan wykonawczy Shadow Burnin V2 L2: offline research-grade candidate

Status planu:

```text
ACCEPTED_FOR_IMPLEMENTATION_PLANNING
```

Ten plan opisuje sekwencje prac potrzebna do doprowadzenia Shadow Burnin V2 do
poziomu L2:

```text
SHADOW_V2_L2_RESEARCH_GRADE_CANDIDATE_OFFLINE_ONLY
```

To nie jest zgoda na L3, live-equivalence, runtime approval, active close ani
strategy unlock. L2 oznacza tylko research-grade candidate dla offline research.

## 1. Cel, naming i twarde granice

Po zakonczeniu tej sekwencji nadal musza pozostac falszywe:

```text
runtime_approval=false
live_equivalence=false
strategy_research_unblocked=false
active_close=false
shadow_close_only=false
```

Nazwy `L2-A0` ... `L2-F` sa stage-labelami wykonawczymi, a nie numerami GitHub
PR. Realne numery PR nada GitHub po PR47. Nie uzywac starych etykiet
`PR44/PR45/PR46` jako nazw galezi ani raportow, zeby nie mieszac z historia
repo.

Start operacyjny:

- baza: clean `main` po merge PR47;
- zweryfikowany merge commit PR47:
  `42b0ccd82210b8d3d32f5ebe291ce546defd2c34`;
- kazdy stage robic na osobnej galezi z clean worktree;
- staging tylko allowlista, nigdy `git add .`;
- kazda zmiana plikow repo wymaga ADR-8D w `docs/ADR/`.

Zakazy globalne:

- nie zmieniac BUY/REJECT;
- nie zmieniac Gatekeeper policy ani progow decyzyjnych;
- nie zmieniac selector runtime;
- nie dotykac TX/Jito/live path;
- nie dotykac R51;
- nie wlaczac active close ani `shadow_close_only`;
- nie podlaczac nowych Shadow V2 evidence fields do `MaterializedFeatureSet`
  jako decision input;
- nie dodawac szerokich nowych NLN subscriptions w tej sekwencji;
- nie uzywac provider-normalized metadata jako pelnego chain-order proof bez
  exact causal join;
- nie traktowac Solana-native `log_index` jak Ethereum `logIndex`;
- nie luzowac `has_complete_chain_order()`.

## 2. Stage L2-A0: account_data_hash raw-bytes source audit

Charakter:

```text
audit-only / report-only
```

Cel: ustalic, gdzie sa raw account bytes i czy nadaja sie do chain-observed
account-state provenance.

Zakres:

- znalezc raw bytes z Geyser/Seer `AccountUpdate`;
- potwierdzic, ze sa to oryginalne account update bytes, nie decoded struct;
- ustalic, gdzie bytes/hash sa tracone: Seer decode, IPC, launcher event,
  `AccountStateUpdate`, `CanonicalPoolState`, entry boundary, exit/path samples;
- sprawdzic pending/replay account update path;
- wskazac dokladne miejsce liczenia BLAKE3;
- wskazac minimalny metadata set:
  `account_pubkey`, owner/program, `slot`, `write_version`, `data_len`,
  `account_data_hash`;
- potwierdzic, czy raw bytes musza byc przenoszone dalej. Domyslnie: nie.

Artefakty:

```text
PLANS/AUDYT/RAPORT_SHADOW_V2_L2_A0_ACCOUNT_DATA_HASH_SOURCE_AUDIT_20260704.md
docs/ADR/ADR_8D_SHADOW_V2_L2_A0_ACCOUNT_DATA_HASH_SOURCE_AUDIT_20260704.md
reports/selector/shadow_v2_l2_a0_account_data_hash_source_matrix.csv
```

CSV minimum:

```text
component,raw_bytes_present,hash_present,write_version_present,account_pubkey_present,retained_to_next_boundary,notes
```

Dozwolone verdicts:

```text
ACCOUNT_DATA_HASH_RAW_BYTES_SOURCE_PRESENT
ACCOUNT_DATA_HASH_SOURCE_PARTIAL_PATH_PRESENT
BLOCKED_ACCOUNT_DATA_HASH_RAW_BYTES_NOT_RETAINED
```

Acceptance:

- raport jednoznacznie wskazuje implementation path dla L2-B;
- raport zabrania hashy z decoded struct;
- brak runtime behavior changes.

## 3. Stage L2-B: account_data_hash propagation

Charakter:

```text
evidence-only implementation
```

Cel: przeniesc BLAKE3 raw account bytes do Shadow V2 evidence.

Kontrakt:

```text
account_data_hash = BLAKE3(original raw account update bytes)
```

Minimalny lancuch:

```text
Seer raw AccountUpdate.data
-> BLAKE3 hash close to ingest
-> IPC account update event
-> launcher AccountUpdateEvent
-> AccountStateUpdate
-> CanonicalPoolState
-> ShadowV2EntryBoundaryPayload
-> PoolStateSampleV2
-> exit/path pool-state samples when causally tied
```

Twarda zasada pamieci:

```text
Nie przenosic raw bytes dalej niz trzeba.
Liczyc BLAKE3 mozliwie blisko ingest.
Dalej przenosic hash + metadata, nie raw data.
```

Wymagane pola evidence:

```text
account_data_hash
account_data_len
source_account_pubkey
source_account_owner_or_program
source_slot
source_write_version
```

Backward compatibility:

- nowe pola addytywne;
- `serde(default)` tam, gdzie stare rekordy moga byc odczytywane;
- missing hash = typed blocker, nie placeholder.

Testy obowiazkowe:

- hash jest liczony z raw bytes;
- hash nie jest liczony z decoded struct, reserves, JSON ani
  `CanonicalPoolState`;
- IPC zachowuje hash/data_len/pubkey/write_version;
- `AccountStateUpdate` i `CanonicalPoolState` zachowuja hash;
- entry boundary dostaje hash z causally tied canonical state;
- exit/path sample dostaje hash tylko przy causally tied pool-state source;
- missing hash emituje typed blocker;
- backward serde test dla rekordow bez nowych pol;
- static guard: evidence nie jest konsumowane przez Gatekeeper/selector/live
  path.

Expected verdict:

```text
L2_B_ACCOUNT_DATA_HASH_PROPAGATED_STILL_L2_BLOCKED
```

## 4. Stage L2-C: Solana temporal source proof closure

Charakter:

```text
temporal semantics + audit closure
```

Cel: rozdzielic transaction-order proof od account-state proof bez falszywego
PASS.

Dwa osobne proofy:

```text
transaction_source_proof_complete != account_state_source_proof_complete
```

Transaction-like events wymagaja:

```text
slot/block_time + source_tx_signature + tx_index + instruction_index
```

Account-state pool-state samples wymagaja:

```text
account_pubkey + slot + write_version + BLAKE3(raw account bytes)
```

Hard fail:

```text
Nie wolno zmienic has_complete_chain_order() tak,
aby account-state proof udawal transaction chain-order.
```

`has_complete_chain_order()` pozostaje backward-compatible i nie jest luzowany.
Dla L2 trzeba dodac osobna Solana-aware audit classification/predicate, ktora
rozroznia transaction proof, account-state proof i `NOT_APPLICABLE`.

`inner_instruction_index`:

- nie mapowac `inner_group_index` domyslnie;
- jesli event moze pochodzic z CPI albo parser nie umie tego rozstrzygnac,
  zostaje `UNKNOWN / not exact`;
- `NOT_APPLICABLE` tylko dla formalnie udowodnionego outer-instruction-only
  source.

`log_index`:

- Solana-native `log_index = NOT_APPLICABLE`;
- `LOG_MESSAGE_INDEX_INTERNAL` jest opcjonalny i tylko jesli parser/indexer
  enumeruje raw `meta.logMessages`;
- `event_seq_in_process`, `event_ordinal`, `ix_count`, `iix_count`,
  `inner_group_index` nie moga byc uzyte jako log/order substitute.

Temporal audit musi raportowac:

```text
temporal_audit_verdict
transaction_source_proof_complete_count
account_state_source_proof_complete_count
unknown_required_source_count
not_applicable_accepted_count
fake_handoff_signature_count
event_seq_chain_order_substitute_count
terminal_truth_derived_count
```

Acceptance:

- no fake handoff signature;
- pool-state samples przechodza przez account-state proof, nie przez fake
  transaction proof;
- transaction-like evidence nadal wymaga transaction metadata;
- terminal truth i derived after-state pozostaja `DERIVED`.

Dozwolone verdicts:

```text
L2_C_TEMPORAL_SOURCE_PROOF_CLOSED_STILL_DENSITY_BLOCKED
BLOCKED_TEMPORAL_TRANSACTION_SOURCE_JOIN
BLOCKED_TEMPORAL_ACCOUNT_STATE_SOURCE_PROOF
BLOCKED_FAKE_SOURCE_JOIN_DETECTED
```

## 5. Stage L2-D: density / horizon / retention contract

Charakter:

```text
metrology contract + audit implementation
```

Cel: formalnie okreslic, ktore horyzonty sa ocenialne dla L2 baseline.

Declared supported horizons dla pierwszego L2 baseline:

```text
2000
3000
10000
30000
120000
```

Dlugie horyzonty:

```text
300000
500000
```

Domyslny status dlugich horyzontow:

```text
NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
```

Twarda zasada:

```text
Pierwszy L2 baseline nie moze failowac przez 300s/500s,
jesli te horyzonty nie zostaly jawnie zadeklarowane.
```

Kontrakt auditowy:

- PASS wymaga coverage dla wszystkich declared supported horizons;
- unsupported/undeclared horizons nie sa blockerem L2 baseline;
- unsupported horizons nie moga byc uzywane do pozytywnego research claim;
- retencja i replay coverage musza pokrywac najwiekszy declared horizon plus
  margines techniczny;
- rozrozniac `PASS`, `FAILED`, `NOT_EVALUABLE`, `UNDECLARED`,
  `HORIZON_UNMATURED`, `CENSORED_PATH`.

Metrics minimum:

```text
horizon_ms
eligible_positions
evaluable_positions
coverage_ratio
samples_per_position_p50
samples_per_position_p90
max_gap_ms_p90
max_gap_ms_max
duplicate_sample_count
non_monotonic_sample_count
censored_count
horizon_unmatured_count
retention_gap_count
verdict
```

Dozwolone verdicts:

```text
L2_D_DENSITY_HORIZON_CONTRACT_PASS_FOR_DECLARED_HORIZONS
BLOCKED_DENSITY_DECLARED_HORIZON_INCOMPLETE
BLOCKED_RETENTION_CONTRACT_INSUFFICIENT
```

## 6. Stage L2-E: Gatekeeper coverage / denominator / starvation audit

Charakter:

```text
audit/metrology only
```

Cel: upewnic sie, ze research sample ma znany denominator i nie jest glodzony
przez Gatekeeper/observation thresholds.

Zakres:

- candidate universe count z event-level denominatora;
- eligible denominator;
- reject reason distribution;
- checkpoint reach count;
- entry `RESEARCH_CANDIDATE` count;
- exit `RESEARCH_CANDIDATE` count;
- `research_candidate_roundtrip_count`;
- complete executable roundtrip count;
- threshold starvation check;
- unknown/generic reason bucket count.

Twardy zakaz:

```text
Jesli Gatekeeper starvation wystapi,
nie wolno w tym samym stage luzowac progow.
```

Wtedy verdict:

```text
BLOCKED_GATEKEEPER_THRESHOLD_STARVATION
```

i osobny plan na policy/gating.

Minimalny CSV:

```text
metric,value,notes
candidate_universe_count,,
eligible_denominator_count,,
gatekeeper_reject_count,,
gatekeeper_reject_reason_top_n,,
checkpoint_reach_count,,
entry_research_candidate_count,,
exit_research_candidate_count,,
research_candidate_roundtrip_count,,
complete_executable_roundtrip_positions,,
threshold_starvation_verdict,,
unknown_reason_count,,
```

Dozwolone verdicts:

```text
GATEKEEPER_DENOMINATOR_COVERAGE_KNOWN
BLOCKED_GATEKEEPER_THRESHOLD_STARVATION
BLOCKED_CANDIDATE_UNIVERSE_DENOMINATOR_UNKNOWN
BLOCKED_UNKNOWN_REJECT_REASON_BUCKETS
```

## 7. Stage L2-F: dedicated research validation run

Charakter:

```text
dedicated research run, not smoke
```

Warunki wejscia:

```text
L2-A0 merged
L2-B merged
L2-C merged
L2-D merged
L2-E merged
clean main
no local config/runtime/report noise staged
```

Run manifest musi zawierac:

```text
run_id
git_head
config_hash
started_at
ended_at
declared_supported_horizons_ms
unsupported_horizons_ms
retention_contract
artifact_paths
schema_manifest_version
```

Final gates:

```text
complete_executable_roundtrip_positions >= 500
entry_RESEARCH_CANDIDATE_count > 0
exit_RESEARCH_CANDIDATE_count > 0
research_candidate_roundtrip_count > 0
temporal_audit_verdict = PASS_TEMPORAL_NO_LOOKAHEAD_AUDIT
density_audit_verdict = PASS_FOR_DECLARED_HORIZONS
manifest_audit_verdict = PASS
replay_lifecycle_verdict = PASS
malformed_rows = 0
unknown_untyped_blockers = 0
fake_handoff_signature_count = 0
event_seq_chain_order_substitute_count = 0
Gatekeeper_coverage = known
threshold_starvation_verdict != BLOCKED
```

`account_data_hash_required_coverage` denominator:

```text
100% over observed account-state boundary samples in L2 scope
derived/not-applicable rows excluded from denominator
legacy/backward-compatible rows excluded or separately reported
```

Dozwolone final verdicts:

```text
SHADOW_V2_L2_RESEARCH_GRADE_CANDIDATE_OFFLINE_ONLY
BLOCKED_L2_SAMPLE_SIZE
BLOCKED_L2_TEMPORAL_OR_HASH_PROVENANCE
BLOCKED_L2_DENSITY_OR_RETENTION
BLOCKED_L2_GATEKEEPER_COVERAGE
BLOCKED_L2_MANIFEST_OR_REPLAY
```

Final flags:

```text
research_grade_candidate = true_for_shadow_v2_offline_research_only
runtime_approval = false
live_equivalence = false
strategy_research_unblocked = false
```

## 8. Wspolne testy i guardy

Minimalny guard dla implementacyjnych stage'y:

```bash
cargo test -p ghost-core shadow_v2 -- --nocapture
cargo test -p ghost-brain shadow_v2_event_order -- --nocapture
cargo test -p ghost-brain shadow_v2_pool_state -- --nocapture
cargo test -p ghost-launcher shadow_v2_event_order -- --nocapture
cargo test -p ghost-launcher shadow_v2_no_decision_consumption_static_guard -- --nocapture
cargo check -p ghost-core
cargo check -p ghost-brain
cargo check -p ghost-launcher
cargo fmt --check
python3 -m py_compile scripts/shadow_v2_temporal_no_lookahead_audit.py
python3 -m py_compile scripts/shadow_v2_path_density_horizon_audit.py
python3 -m py_compile scripts/shadow_v2_manifest_audit.py
git diff --check
git diff --cached --check
```

Hard fail dla calej sekwencji:

```text
hash liczony z decoded struct
raw bytes przeniesione do hot canonical state bez koniecznosci z L2-A0
fake source signature uzyte jako proof
account_state_source_proof_complete zmieszane z transaction_source_proof_complete
has_complete_chain_order() loosened
event_seq_in_process uzyte jako ordering proof
inner_group_index uzyty jako exact inner_instruction_index bez kontraktu
CPI-ambiguous inner index oznaczony jako NOT_APPLICABLE
log_index traktowany jako Solana-native missing provider field
300s/500s blokuja baseline mimo braku deklaracji
Gatekeeper starvation naprawiane przez tuning progow w tym samym stage
Gatekeeper/BUY/REJECT/selector/live path changed
terminal truth no longer DERIVED
runtime_approval/live_equivalence granted
```

## 9. Zalozenia

- PR47 jest merged i `main` po
  `42b0ccd82210b8d3d32f5ebe291ce546defd2c34` jest baza.
- L2 baseline dotyczy offline research only.
- L2 nie dowodzi live fills, realized slippage, quote/fill divergence ani L3
  live-equivalence.
- Provider Enhanced Streams moga byc osobnym lifecycle/risk/cross-venue
  trackiem, ale nie sa wymagane do `account_data_hash` L2 baseline.
- Jezeli L2-A0 wykaze brak raw bytes w wymaganym miejscu, sekwencja zatrzymuje
  sie na blockerze zamiast implementowac pseudo-proof.

## 10. Delegation trace

```yaml
task_classification: "cross-cutting Shadow V2 L2 research-grade execution plan"
routing_performed: true
primary_specialist: "Ghost Runtime Coordinator"
supporting_specialists_considered:
  - "Seer Ingest Event Integrity Specialist"
  - "Decision Logging Replay Analyst"
  - "SSOT Feature Materialization Guardian"
  - "Solana Execution Path Engineer"
  - "Gatekeeper Policy Auditor"
specialist_docs_loaded:
  - "docs/agents/ghost-runtime-coordinator.md"
  - "docs/agents/seer-ingest-event-integrity-specialist.md"
  - "docs/agents/decision-logging-replay-analyst.md"
  - "docs/agents/ssot-feature-materialization-guardian.md"
specialist_docs_not_loaded:
  - name: "gatekeeper-policy-auditor.md"
    reason: "plan forbids Gatekeeper policy changes; only denominator/starvation audit is included"
  - name: "solana-execution-path-engineer.md"
    reason: "L2 excludes TX/Jito/live equivalence and does not mutate execution path"
skills_used:
  - "ghost-execution"
  - "solana-pumpfun-architect"
  - "statistical-research-engine"
  - "rust-master"
  - "large-data-analytics"
fast_path_used: false
contracts_checked:
  - "SSOT / MaterializedFeatureSet unchanged"
  - "Seer/Geyser raw account bytes provenance"
  - "AccountStateUpdate and CanonicalPoolState evidence propagation"
  - "Shadow/live boundary"
  - "DecisionLogger/replay/audit evidence"
  - "Solana transaction proof vs account-state proof separation"
  - "Temporal no-lookahead"
  - "Density/horizon/retention"
  - "Gatekeeper denominator coverage"
unresolved_routing_uncertainty:
  - "Exact field names for account-state proof remain to be finalized after L2-A0"
  - "300s/500s remain outside baseline unless explicitly declared for a later long-horizon run"
```
