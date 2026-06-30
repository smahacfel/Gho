# ADR-8D: Shadow Burnin V2 PR12/PR13 Fidelity Validation Plan and Legacy Downgrade Enforcement

Data: 2026-06-30

Status:

```text
PR12_PR13_READY_FOR_REVIEW
```

## D1. Problem

Po PR10/PR11 Shadow V2 ma kontrakt manifestów i inert logging-only config.
Nadal brakowało dwóch zamykających warstw planu remediation:

- PR12: formalnego planu fidelity validation burnin, który definiuje bramki
  research-grade bez uruchamiania runu;
- PR13: enforcement layer, który zabrania traktowania raportów Shadow V1 jako
  live-equivalent PnL albo executable fill proof.

Bez PR12 przyszły burnin mógłby zostać błędnie zinterpretowany jako strategy
proof. Bez PR13 stare raporty mogłyby dalej być cytowane bez downgrade labels.

## D2. Decision

Dodajemy PR12/PR13 jako statyczne, offline kontrakty:

- `PLANS/AUDYT/PLAN_SHADOW_V2_FIDELITY_VALIDATION_BURNIN_PR12_20260630.md`;
- `configs/rollout/shadow_v2_fidelity_validation_burnin_plan.toml`;
- `scripts/shadow_v2_validation_burnin_plan_audit.py`;
- `scripts/test_shadow_v2_validation_burnin_plan_audit.py`;
- `PLANS/AUDYT/RAPORT_SHADOW_V2_LEGACY_DOWNGRADE_ENFORCEMENT_PR13_20260630.md`;
- `scripts/shadow_v2_legacy_downgrade_audit.py`;
- `scripts/test_shadow_v2_legacy_downgrade_audit.py`;
- aktualizacje spec, gates, workbreakdown, schema manifest, downgrade matrix
  i risk register.

PR12/PR13 nie uruchamiają runu, nie czytają raw run JSONL jako dowodu, nie
zmieniają runtime i nie odblokowują strategii.

## D3. Evidence

PR12 evidence:

- plan PR12 w `PLANS/AUDYT`;
- static TOML plan w `configs/rollout`;
- audit script i fixture tests;
- gates `GATE_PR12_*` w `reports/selector/shadow_v2_acceptance_gates.csv`.

PR13 evidence:

- downgrade enforcement report w `PLANS/AUDYT`;
- `reports/selector/shadow_v2_legacy_downgrade_matrix.csv`;
- audit script i fixture tests;
- gates `GATE_PR13_*` w `reports/selector/shadow_v2_acceptance_gates.csv`.

Root evidence pozostaje:

- P0 Shadow Burnin Fidelity Audit;
- downgrade impact pack;
- Shadow V2 remediation plan;
- PR1-PR11 merged contracts.

## D4. Root Cause

Shadow V1 zawiera znany mismatch lifecycle/replay i nie udowadnia live fills.
Samo istnienie Shadow V2 komponentów PR1-PR11 nie wystarcza do research-grade:
potrzebny jest osobny fidelity validation burnin z manifestami, density checks,
reconciliation checks i golden traces.

Jednocześnie stare raporty muszą zostać zachowane, ale z trwałym downgrade
contract, bo usunięcie V1 utrudniłoby forensics, a brak labeli groziłby
fałszywym cytowaniem jako live proof.

## D5. Corrective Action

PR12:

- definiuje `validation_mode=FIDELITY_ONLY`;
- wymusza `PLAN_ONLY` i `run_start_allowed=false`;
- wymaga dense/standard/long path modes;
- wymaga horizon checks 2s, 3s, 10s, 30s, 120s, 300s, 500s;
- wymaga pre-run i post-run manifestów;
- wymaga gates entry/exit reconstruction, reconciliation, density, temporal,
  clock-domain, event-order, fixtures i manifests;
- blokuje strategy/RCE/selector/edge/runtime approval proof.

PR13:

- utrzymuje macierz downgrade;
- wymaga labeli dla ORG-A0, R48/R2, TSV2, EIX, RTP, RUG, RCE, R51,
  lifecycle V1 i `shadow_exit_replay_v1`;
- blokuje live-equivalent wording w `allowed_use`;
- wymaga jawnego tekstu:
  `Previous reports must not be cited as proof of live PnL`;
- zachowuje V1 jako diagnostic/component evidence, nie jako truth source.

## D6. Rejected Alternatives

Odrzucono:

- uruchomienie validation burnin w PR12;
- traktowanie PR12 jako strategy proof;
- traktowanie PR13 jako usunięcia V1;
- uznanie Shadow V1 za live-equivalent po samym dodaniu V2;
- merge raw JSONL/logs jako dowodu;
- dotykanie R51;
- zmiany BUY/REJECT, Gatekeeper, selector, TX/Jito/live path, close path.

## D7. Consequences

Po PR12/PR13:

- można osobno zatwierdzić przyszły fidelity validation burnin;
- wiadomo, jakie bramki muszą przejść, aby mówić `SHADOW_V2_RESEARCH_GRADE`;
- stare raporty mają enforceable downgrade labels;
- V1 never live-equivalent;
- R51 remains ACTIVE_PARTIAL_DIAGNOSTIC_ONLY;
- bez PR14 max verdict pozostaje `SHADOW_V2_RESEARCH_GRADE_ONLY`;
- runtime approval, `shadow_close_only` approval i active close approval
  pozostają false.

## D8. Verification

Wymagane lokalne sprawdzenia PR12/PR13:

```text
python3 -m py_compile scripts/shadow_v2_validation_burnin_plan_audit.py scripts/test_shadow_v2_validation_burnin_plan_audit.py scripts/shadow_v2_legacy_downgrade_audit.py scripts/test_shadow_v2_legacy_downgrade_audit.py
python3 scripts/shadow_v2_validation_burnin_plan_audit.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_shadow_v2_validation_burnin_plan_audit.py
python3 scripts/shadow_v2_legacy_downgrade_audit.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_shadow_v2_legacy_downgrade_audit.py
python3 scripts/shadow_v2_manifest_audit.py
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
