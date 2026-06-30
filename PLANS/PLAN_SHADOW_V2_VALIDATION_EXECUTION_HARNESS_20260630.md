# Plan Shadow V2 Validation Execution Harness 2026-06-30

## 1. Cel

Ten etap implementuje pierwszy minimalny, logging-only harness, ktory pozwala
produkować evidence Shadow V2 bez uruchamiania strategii i bez zmiany decyzji
runtime.

Zakres pozostaje administracyjno-pomiarowy:

- `shadow_v2_burnin.enabled=false` zachowuje dotychczasowe zachowanie procesu;
- `shadow_v2_burnin.enabled=true` uruchamia tylko walidacyjny preflight i
  logging-only harness;
- harness moze fail-closed tylko jako
  `SHADOW_V2_VALIDATION_PREFLIGHT_FAILED`;
- fail preflight nie jest Gatekeeper failure, BUY/REJECT failure ani selector
  failure;
- Shadow V2 artifacts nie sa konsumowane przez policy, selector ani execution
  path.

Plan verdict:

```text
PR15_LOGGING_ONLY_HARNESS_IMPLEMENTED_LOCAL
```

Nie jest to validation burnin, strategy proof, RCE proof, selector proof ani
edge proof.

## 2. Granice

In scope:

- config consumption tylko dla `shadow_v2_burnin` i tylko w trybie
  `logging_only_validation`;
- pre-run manifest strict preflight przed startem walidacyjnego harnessu;
- canonical writer wrapper dla `shadow_position_event_v2.jsonl`;
- derived snapshot writers dla `shadow_replay_v2.jsonl` i
  `shadow_lifecycle_v2.jsonl`;
- concrete row wrapper dla `shadow_path_density_v2.jsonl`;
- shutdown/post-run manifest generation pass bez `--strict`;
- osobny shutdown/post-run strict verification pass;
- outcome, ktory rozdziela canonical durable success od derived artifact write
  failure;
- testy potwierdzajace brak Pythona w hot path i brak konsumpcji Shadow V2 w
  decision/execution source.

Out of scope:

- BUY/REJECT changes;
- Gatekeeper policy changes;
- selector runtime changes;
- TX/Jito/live path changes;
- `shadow_close_only`;
- active close;
- strategy tuning;
- RCE proof;
- start runa walidacyjnego;
- R51 touch;
- stage raw JSONL, logs albo runtime artifacts.

## 3. Runtime Boundary

### Disabled

Gdy `shadow_v2_burnin.enabled=false`:

- harness nie jest inicjalizowany;
- preflight manifest audit nie jest uruchamiany;
- post-run manifest generation nie jest uruchamiany;
- event path nie wywoluje Pythona;
- runtime zachowuje sie jak dotychczas.

### Enabled

Gdy `shadow_v2_burnin.enabled=true`:

- config musi miec `mode=logging_only_validation`;
- config musi miec `logging_only=true`;
- config musi miec `runtime_approval=false`;
- config musi miec `shadow_close_only_approval=false`;
- config musi miec `active_close_approval=false`;
- config musi miec `strategy_proof_enabled=false`;
- wymagane sa sciezki:
  - `scope_root_path`;
  - `pre_run_manifest_path`;
  - `post_run_manifest_path`;
  - `canonical_event_stream_path`;
  - `replay_v2_path`;
  - `lifecycle_v2_path`;
  - `path_density_v2_path`.

Brak pre-run manifestu lub nieudany strict manifest audit blokuje start trybu
walidacyjnego jako `SHADOW_V2_VALIDATION_PREFLIGHT_FAILED`.

## 4. Artifact Contract

PR15 produkuje lub przygotowuje writer dla:

- `shadow_position_event_v2.jsonl`;
- `shadow_replay_v2.jsonl`;
- `shadow_lifecycle_v2.jsonl`;
- `shadow_path_density_v2.jsonl`.

`shadow_replay_v2` i `shadow_lifecycle_v2` sa append-only derived snapshots,
nie canonical truth.

Snapshot ID scheme:

```text
replay_v2:{position_id}:{source_canonical_high_watermark}
lifecycle_v2:{position_id}:{source_canonical_high_watermark}
```

Kazdy derived row musi wskazywac:

- `source_event_ids`;
- `source_canonical_high_watermark`;
- canonical stream ref;
- limitation, ze derived view nie jest canonical terminal truth.

## 5. Path Density Row Schema

`shadow_path_density_v2.jsonl` nie moze zawierac golej ewaluacji horyzontu.
Wymagany jest wrapper:

```text
schema
schema_version
run_id
session_id
position_id
pool_id
base_mint
canonical_event_stream_ref
source_path_sample_event_ids
source_canonical_high_watermark
horizon_ms
verdict
path_points
coverage_points
replay_horizon_ms
first_path_point_age_ms
median_interval_ms
p90_interval_ms
max_interval_ms
duplicate_age_count
non_monotonic_input
truncated
limitations
created_at_wall_ms
```

Brak pokrycia sciezki musi byc zapisany jako `NOT_EVALUABLE_*`, nie jako
milczacy sukces.

## 6. Write Semantics

Canonical write jest osobny od derived writes.

Wymagany outcome:

```text
ShadowV2HarnessAppendOutcome
canonical_write: Ok / Err
replay_write: Ok / Err / Skipped
lifecycle_write: Ok / Err / Skipped
density_write: Ok / Err / Skipped
validation_evidence_status
```

Zasady:

- canonical JSONL append musi sie udac przed commit do in-memory canonical
  stream;
- jezeli canonical write failuje, derived writes sa skipped;
- jezeli canonical write sie uda, a derived write failuje, canonical event
  pozostaje durable;
- taki przypadek dostaje status
  `DERIVED_ARTIFACT_WRITE_FAILED` albo `DENSITY_WRITE_FAILED`;
- runtime decision pozostaje niezmieniona.

## 7. Manifest Audit

Python manifest audit jest dozwolony tylko:

- przy preflight startu trybu walidacyjnego;
- przy shutdown/post-run.

Python manifest audit jest zabroniony:

- per event;
- per slot;
- per tx;
- per position update;
- w hot decision path.

Post-run shutdown guard:

1. flush/zamkniecie writerow harnessu;
2. post-run generation pass bez `--strict`;
3. osobny strict verification pass;
4. failure oznacza `SHADOW_V2_POST_RUN_MANIFEST_AUDIT_FAILED`;
5. failure nie zmienia BUY/REJECT i nie ustawia zadnych approval flags.

Generation pass:

```bash
python3 scripts/shadow_v2_manifest_audit.py \
  --scope-root <scope_root> \
  --manifest-phase post_run \
  --run-id <run_namespace_or_session_id> \
  --write-manifest <post_run_manifest_path> \
  --schema-manifest reports/selector/shadow_v2_required_schema_manifest.csv \
  --acceptance-gates reports/selector/shadow_v2_acceptance_gates.csv
```

Verification pass:

```bash
python3 scripts/shadow_v2_manifest_audit.py \
  --scope-root <scope_root> \
  --manifest-phase post_run \
  --schema-manifest reports/selector/shadow_v2_required_schema_manifest.csv \
  --acceptance-gates reports/selector/shadow_v2_acceptance_gates.csv \
  --strict
```

## 8. Minimal Runtime Wiring

PR15 moze emitowac minimalny `shadow_position_v2` po accepted shadow handoff,
ale tylko jako evidence:

- `simulation_level=MARK_ONLY`;
- `measurement_grade=DIAGNOSTIC_ONLY`;
- limitation `PR15_MINIMAL_POSITION_CREATED_ONLY`;
- limitation `NO_ENTRY_FILL_EXIT_FILL_OR_PATH_INFERENCE_IN_PR15`;
- limitation `SHADOW_V2_RECORD_NOT_CONSUMED_BY_DECISIONS`.

PR15 nie inferuje entry fill, exit fill, path samples ani terminal truth z
brakujacych danych. Brak evidence ma zostac zapisany jako `BLOCKED_BY_DATA`,
`NOT_EVALUABLE` albo pominięty z limitation, nie jako sukces.

## 9. Acceptance Gates

Wymagane bramki:

- `GATE_PR15_ENABLED_FALSE_NO_BEHAVIOR_CHANGE`;
- `GATE_PR15_ENABLED_TRUE_PREFLIGHT_FAIL_CLOSED`;
- `GATE_PR15_NO_DECISION_CONSUMPTION`;
- `GATE_PR15_NO_HOT_PATH_PYTHON`;
- `GATE_PR15_CANONICAL_WRITER_DURABILITY`;
- `GATE_PR15_CANONICAL_DERIVED_FAILURE_SEPARATION`;
- `GATE_PR15_REPLAY_DERIVED_FROM_CANONICAL`;
- `GATE_PR15_LIFECYCLE_DERIVED_FROM_CANONICAL`;
- `GATE_PR15_PATH_DENSITY_ROW_SCHEMA`;
- `GATE_PR15_POST_RUN_GENERATE_THEN_STRICT_VERIFY`;
- `GATE_PR15_NO_RAW_JSONL_OR_LOG_STAGED`.

## 10. Verification

Minimalne lokalne komendy:

```bash
cargo test -p ghost-brain shadow_v2_config -- --nocapture
cargo test -p ghost-brain shadow_v2_validation_harness -- --nocapture
cargo test -p ghost-launcher shadow_v2 -- --nocapture
cargo fmt --check
python3 scripts/shadow_v2_manifest_audit.py --help
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_shadow_v2_manifest_audit.py
git diff --check
git diff --cached --name-only
```

Forbidden staged guard:

```bash
git diff --cached --name-only | rg 'runtime\.log|\.jsonl$|^logs/|datasets/events|__pycache__|shadow_lifecycle|shadow_exit_replay|gatekeeper_v2_decisions|reports/selector/shadow-burnin-v3-r51' && { echo "BLOCK forbidden staged file"; exit 1; } || true
```

## 11. Final Boundary

Po PR15 nadal obowiazuje:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
strategy_research_unblocked=false
```

PR15 nie przyznaje:

- `SHADOW_V2_RESEARCH_GRADE`;
- `SHADOW_V2_LIVE_EQUIVALENCE_GRADE`;
- RCE approval;
- selector approval;
- active close approval.

Nastepny etap po review PR15 to osobna decyzja operatora o pierwszym
fidelity-only validation burnin. PR15 sam tego runa nie startuje.
