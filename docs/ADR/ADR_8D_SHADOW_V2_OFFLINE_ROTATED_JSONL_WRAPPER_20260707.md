# ADR-8D: Shadow V2 offline wrapper rotated JSONL compatibility

Data: 2026-07-07
Status: Accepted for implementation review
Scope: offline audit/wrapper compatibility only
Decision: `ROTATED_JSONL_ALL_PARTS_OFFLINE_WRAPPER_COMPATIBILITY_FIXED`

## D1 - Problem

R4 strict wrapper evaluation was falsely blocked by insufficient sample size because offline audit helpers read only the active base artifact `shadow_position_event_v2.jsonl`. The canonical stream for R4 had already rotated most rows into `shadow_position_event_v2.part-000001.jsonl`, so the wrapper-visible sample was much smaller than the real local evidence stream.

## D2 - Context

The affected path is offline-only:

- `scripts/shadow_v2_offline_audit_common.py`;
- `scripts/shadow_v2_l2_f_research_validation_run.py`;
- `scripts/shadow_v2_path_density_horizon_audit.py`.

The fix must not start a collection run, delete artifacts, compress artifacts, change Gatekeeper policy, change BUY/REJECT behavior, or alter runtime execution. Existing JSONL rotation manifests can validate and report discovered rotation metadata, but offline reading must not depend on the manifest being present or complete.

## D3 - Decision

Add a shared `artifact_jsonl_paths(scope_root, artifact_name)` helper that returns the logical JSONL stream in this order:

- `*.part-000001.jsonl`;
- `*.part-000002.jsonl`;
- additional sorted part files;
- active base `*.jsonl`.

Wire the common helpers for these artifacts through that path list:

- `shadow_position_event_v2.jsonl`;
- `shadow_replay_v2.jsonl`;
- `shadow_lifecycle_v2.jsonl`;
- `shadow_path_density_v2.jsonl`.

Also wire density scans in `shadow_v2_path_density_horizon_audit.py` and the L2-F wrapper position-level density gate through the shared all-parts density iterator. Add rotation reporting to the L2-F wrapper output via `rotated_artifacts`, including manifest presence, manifest/discovered part mismatches, and `read_depends_on_manifest=false`.

## D4 - Rejected Alternatives

Rejected:

- reading only `shadow_artifact_rotation_manifest_v2.jsonl` as the source of truth for part discovery;
- continuing to read only base JSONL artifacts in wrapper gates;
- changing runtime JSONL emission or rotation behavior;
- changing Gatekeeper policy, BUY/REJECT logic, selector runtime, TX/Jito/live path, provider streams, or thresholds;
- deleting, compressing, or regenerating collection artifacts.

## D5 - Consequences

Positive consequences:

- R4 canonical and replay rotated parts are included in offline wrapper evidence;
- no-parts scopes retain legacy base-only behavior;
- malformed row counts include rotated parts;
- density audits use the same all-parts compatibility path as canonical/replay/lifecycle helpers;
- wrapper output reports whether rotation manifest metadata was available without depending on it for reads.

Remaining limitations:

- manifest audit still reports physical files individually; it does not collapse parts into a single logical stream;
- this ADR does not change runtime artifact rotation, compression, retention, or collection behavior;
- this fix does not prove any live execution or live-equivalence claim.

## D6 - Invariants

Preserved invariants:

- no collection run started;
- no raw source artifacts deleted;
- no raw source artifacts compressed;
- no Gatekeeper policy change;
- no BUY/REJECT logic change;
- no runtime execution change;
- no selector runtime change;
- no TX/Jito/live path change;
- no provider stream change;
- no threshold change;
- no `MaterializedFeatureSet` or decision input change;
- shadow/live boundary remains unchanged;
- approval flags remain false in wrapper output.

## D7 - Validation

Executed validation set:

- `python3 -m py_compile scripts/shadow_v2_offline_audit_common.py` - PASS;
- `python3 -m py_compile scripts/shadow_v2_l2_f_research_validation_run.py` - PASS;
- `python3 -m py_compile scripts/shadow_v2_path_density_horizon_audit.py` - PASS;
- `python3 tests/test_shadow_v2_offline_audit_common.py` - PASS;
- `python3 tests/test_shadow_v2_l2_f_research_validation_run.py` - PASS;
- R4 strict wrapper rewrap - PASS;
- R5 strict wrapper regression rewrap - PASS.

R4 final evidence:

- final verdict: `L2_RESEARCH_GRADE_CANDIDATE_OFFLINE_ONLY`;
- evidence-complete positions: `1560`;
- blockers: `[]`;
- rotated canonical parts: `shadow_position_event_v2.part-000001.jsonl` before base;
- rotated replay parts: `shadow_replay_v2.part-000001.jsonl` before base.

R5 final evidence:

- final verdict: `L2_RESEARCH_GRADE_CANDIDATE_OFFLINE_ONLY`;
- evidence-complete positions: `1347`;
- blockers: `[]`;
- no rotated parts discovered;
- base-only fallback preserved.

## D8 - Final

Final verdict:

`ROTATED_JSONL_ALL_PARTS_OFFLINE_WRAPPER_COMPATIBILITY_FIXED`

The previous R4 `80` evidence-complete result was a false negative caused by base-only offline iteration over a rotated local JSONL stream.
