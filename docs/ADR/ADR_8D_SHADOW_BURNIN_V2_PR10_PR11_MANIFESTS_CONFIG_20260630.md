# ADR-8D: Shadow Burnin V2 PR10/PR11 Manifests and Logging-Only Config

Data: 2026-06-30

Status:

```text
PR10_PR11_COMPLETED_ON_PR_BRANCH_PENDING_REVIEW
```

## D1. Problem

Shadow Burnin Simulation V2 ma już side-by-side kontrakty PR1-PR9 dla schemy,
canonical event stream, pool-state provenance, price reconstruction, entry/exit
fill, path sampling oraz derived replay/lifecycle. Nadal brakowało dwóch
warstw operacyjnych wymaganych przez plan remediation:

- PR10: jawnego kontraktu manifestów evidence przed i po przyszłym validation
  burninie;
- PR11: inert config surface dla przyszłego logging-only validation burnin,
  disabled by default i bez runtime approval.

Bez PR10 nie ma gwarancji, że przyszły burnin zachowa sha256, row counts,
schema coverage i kompletność artefaktów. Bez PR11 nie ma kontrolowanego,
serde-compatible sposobu opisania przyszłego Shadow V2 validation profile bez
ryzyka przypadkowego runtime enablement.

## D2. Decision

Dodajemy PR10/PR11 jako kontrakty side-by-side:

- offline script `scripts/shadow_v2_manifest_audit.py`;
- deterministic fixture tests `scripts/test_shadow_v2_manifest_audit.py`;
- artifact contract CSV `reports/selector/shadow_v2_manifest_artifact_contract.csv`;
- inert config section `shadow_v2_burnin` w `GhostBrainConfig`;
- logging-only rollout contract
  `configs/rollout/ghost_brain_shadow_v2_validation_logging_only.toml`;
- aktualizacje spec, acceptance gates, workbreakdown, schema manifest i risk
  register.

PR10/PR11 nie aktywują żadnego runtime path. Config jest walidowany i
serde-compatible, ale nie jest konsumowany przez writer, Gatekeeper, selector,
TX/Jito/live path ani lifecycle.

## D3. Evidence

Nowe dowody PR10:

- `scripts/shadow_v2_manifest_audit.py`
- `scripts/test_shadow_v2_manifest_audit.py`
- `reports/selector/shadow_v2_manifest_artifact_contract.csv`
- `reports/selector/shadow_v2_required_schema_manifest.csv`
- `reports/selector/shadow_v2_acceptance_gates.csv`

Nowe dowody PR11:

- `ghost-brain/src/config/ghost_brain_config.rs`
- `configs/rollout/ghost_brain_shadow_v2_validation_logging_only.toml`
- `docs/SPEC/SHADOW_BURNIN_V2_SIMULATION_CONTRACT_20260629.md`

Root evidence pozostaje bez zmian:

- `PLANS/AUDYT/RAPORT_SHADOW_BURNIN_FIDELITY_AUDIT_20260629.md`
- `docs/ADR/ADR_8D_SHADOW_BURNIN_FIDELITY_AUDIT_20260629.md`
- `PLANS/AUDYT/RAPORT_SHADOW_FIDELITY_DOWNGRADE_IMPACT_20260629.md`
- `docs/ADR/ADR_8D_SHADOW_FIDELITY_DOWNGRADE_IMPACT_20260629.md`
- `reports/selector/shadow_fidelity_claim_evidence_matrix.csv`

## D4. Root Cause

Shadow V1 remediation nie może być uznana za research-grade bez kompletnego
manifestu evidence. Same JSONL lub raporty po runie nie wystarczają, jeżeli nie
ma durable inventory z sha256, row counts, malformed-row counts, schema
coverage i jawnie oznaczonymi brakami.

Konfiguracja przyszłego validation burnin musi być oddzielona od runtime
approval. W przeciwnym razie istnieje ryzyko, że config validation zostanie
błędnie zinterpretowany jako strategy proof albo live-equivalence proof.

## D5. Corrective Action

PR10 ustanawia:

- `shadow_v2_evidence_manifest_v1`;
- `shadow_v2_artifact_manifest_entry_v1`;
- manifest phase `pre_run` / `post_run`;
- required artifact contract CSV;
- offline sha256, line count, JSONL row count, malformed JSONL row count i
  schema coverage;
- strict-mode blockers dla brakujących artefaktów, malformed JSONL i symlinków;
- zasadę `raw_jsonl_git_staging_allowed=false`.

PR11 ustanawia:

- `ShadowV2BurninConfig`;
- `ShadowV2BurninMode`;
- default `enabled=false`;
- partial TOML loader tylko dla `[shadow_v2_burnin]`;
- walidację fail-closed dla `runtime_approval`, `shadow_close_only_approval`,
  `active_close_approval`, strategy/RCE/selector/edge proof flags;
- logging-only rollout TOML bez podłączenia do runtime.

## D6. Rejected Alternatives

Odrzucono:

- staging raw JSONL/logs jako dowodu PR10;
- cleanup evidence przed manifestem;
- traktowanie manifestu jako strategy proof;
- traktowanie logging-only config jako runtime approval;
- uruchamianie PR12 validation burnin w PR10/PR11;
- podpinanie configu do BUY/REJECT, Gatekeeper policy, selector runtime,
  TX/Jito/live path albo close path.

## D7. Consequences

Po PR10/PR11 można przygotować PR12 fidelity validation burnin plan z jasnym
manifest boundary. Nie można jeszcze twierdzić:

- `SHADOW_V2_RESEARCH_GRADE`;
- `SHADOW_V2_LIVE_EQUIVALENCE_GRADE`;
- runtime approval;
- shadow_close_only approval;
- active close approval;
- RCE proof;
- selector proof;
- edge proof.

R51 pozostaje `DIAGNOSTIC_ONLY` / `ACTIVE_PARTIAL` zależnie od stanu artefaktów
i nie jest strategią ani proofem runtime.

## D8. Verification

Wymagane lokalne sprawdzenia PR10/PR11:

```text
python3 -m py_compile scripts/shadow_v2_manifest_audit.py
python3 scripts/shadow_v2_manifest_audit.py --help
python3 scripts/shadow_v2_manifest_audit.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_shadow_v2_manifest_audit.py
cargo test -q -p ghost-brain shadow_v2_config
cargo test -q -p ghost-brain shadow_v2
cargo fmt --check
git diff --check
git diff --cached --check
forbidden staged-file guard
```

Runtime boundary:

```text
NO_RUNTIME_SEMANTICS_CHANGED
NO_RUN_STARTED
NO_R51_TOUCH
NO_BUY_REJECT_CHANGE
NO_GATEKEEPER_POLICY_CHANGE
NO_SELECTOR_RUNTIME_CHANGE
NO_TX_JITO_LIVE_PATH_CHANGE
NO_SHADOW_CLOSE_ONLY_ENABLEMENT
NO_ACTIVE_CLOSE_ENABLEMENT
```
