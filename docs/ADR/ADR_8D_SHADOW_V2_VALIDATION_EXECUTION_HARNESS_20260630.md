# ADR-8D: Shadow V2 Validation Execution Harness

Data: 2026-06-30

Status:

```text
PR15_IMPLEMENTED_LOCAL_PENDING_REVIEW
```

## D1. Problem

Shadow V2 po PR1-PR14 mial kontrakty, typy, manifesty, downgrade boundaries i
calibration contract, ale runtime nadal nie produkowal prawdziwego Shadow V2
evidence. Bez osobnego logging-only harnessu kontrakty pozostawaly inert i nie
mozna bylo wykonac fidelity validation burnin.

Ryzyko tego etapu polega na tym, ze pierwszy kontakt z runtime initialization
moglby niechcacy zmienic BUY/REJECT, Gatekeeper policy, selector runtime albo
TX/Jito/live path.

## D2. Decision

Wprowadzono minimalny Shadow V2 validation execution harness:

- domyślnie disabled;
- aktywny tylko przy `shadow_v2_burnin.enabled=true`;
- dozwolony tylko dla `mode=logging_only_validation`;
- logging-only;
- bez konsumpcji przez decision/execution path;
- z partial loaderem `[shadow_v2_burnin]`, ktory nie zalezy od sukcesu pelnego
  `GhostBrainConfig`;
- z pre-run strict manifest preflight;
- z canonical writer wrapperem;
- z derived replay/lifecycle snapshots;
- z concrete `shadow_path_density_v2` row wrapper;
- z post-run generation pass bez `--strict`, report CSV generation i osobnym
  strict verification pass.

`shadow_v2_burnin.enabled=false` zachowuje dotychczasowe zachowanie procesu.
`shadow_v2_burnin.enabled=true` moze fail-closed tylko jako
`SHADOW_V2_VALIDATION_PREFLIGHT_FAILED`.

## D3. Evidence

Zmiany kodowe:

- `ghost-brain/src/config/ghost_brain_config.rs`
- `ghost-brain/src/guardian/post_buy/shadow_v2.rs`
- `ghost-launcher/src/main.rs`
- `ghost-launcher/src/components/post_buy_runtime.rs`
- `ghost-launcher/tests/post_buy_runtime_integration.rs`
- `configs/rollout/ghost_brain_shadow_v2_validation_logging_only.toml`

Zmiany kontraktowe:

- `PLANS/PLAN_SHADOW_V2_VALIDATION_EXECUTION_HARNESS_20260630.md`
- `docs/SPEC/SHADOW_BURNIN_V2_SIMULATION_CONTRACT_20260629.md`
- `reports/selector/shadow_v2_required_schema_manifest.csv`
- `reports/selector/shadow_v2_acceptance_gates.csv`

Nowe typy/semantyki:

- `ShadowV2ValidationHarness`
- `ShadowV2ValidationHarnessConfig`
- `ShadowV2HarnessAppendOutcome`
- `ShadowV2WriteStatus`
- `ShadowV2ValidationEvidenceStatus`
- `ShadowPathDensityV2`
- `source_canonical_high_watermark` dla derived replay/lifecycle.

## D4. Root Cause

Shadow V2 wymagal pierwszego bezpiecznego runtime-adjacent writer layer. Sam
kontrakt schema nie wystarcza, bo nie tworzy evidence. Jednoczesnie nie wolno
bylo podlaczyc Shadow V2 do decyzji ani wykonania, bo Shadow V2 nie przeszedl
jeszcze validation burnin.

## D5. Corrective Action

Zaimplementowano:

- wymagane config fields `scope_root_path` i `path_density_v2_path`;
- validation preflight w launcher main tylko dla enabled Shadow V2 burnin;
- niezalezne ladowanie `[shadow_v2_burnin]`, dzieki ktoremu unrelated full
  config error nie moze cicho pominac `enabled=true`;
- manifest audit przez Python tylko przy start/shutdown;
- manifest generation traktuje `--write-manifest` i `--write-report-csv` jako
  artefakty generowane w tym samym przebiegu, wiec manifest nie moze sam siebie
  zapisac jako `BLOCKED` przez brak `post_run_manifest.json`;
- canonical append z derived replay/lifecycle/density writes;
- outcome oddzielajacy canonical durable success od derived artifact failure;
- minimalne runtime wiring emitujace tylko diagnostic `shadow_position_v2` po
  accepted shadow handoff;
- limitations wykluczajace entry fill, exit fill, path inference i decision
  consumption w PR15;
- testy no-hot-path-Python i no-decision-consumption.

## D6. Rejected Alternatives

Odrzucono:

- uruchomienie validation burnin w tym PR;
- produkcje entry fill, exit fill lub path samples z niepelnych runtime danych;
- traktowanie derived replay/lifecycle jako canonical truth;
- odwracanie canonical eventu po derived write failure;
- uruchamianie Python manifest audit per event;
- wlaczenie harnessu bez pre-run manifestu;
- podlaczenie Shadow V2 artifacts do Gatekeeper, selector, BUY/REJECT lub
  TX/Jito/live path.

## D7. Consequences

Po tej zmianie mozna reviewowac pierwszy logging-only evidence producer dla
Shadow V2. Nadal nie wolno traktowac tego jako research-grade ani
live-equivalence-grade.

Obowiazujace flagi:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
strategy_research_unblocked=false
```

Maksymalny stan bez osobnego fidelity validation burnin:

```text
CONTRACT_READY / RESEARCH_GRADE_NOT_GRANTED
```

Maksymalny stan bez realnego PR14 live-confirmed calibration dataset:

```text
SHADOW_V2_RESEARCH_GRADE_ONLY
```

## D8. Verification

Wymagane lokalne sprawdzenia:

```text
cargo test -p ghost-brain shadow_v2_config -- --nocapture
cargo test -p ghost-brain shadow_v2_validation_harness -- --nocapture
cargo test -p ghost-launcher shadow_v2 -- --nocapture
cargo fmt --check
python3 scripts/shadow_v2_manifest_audit.py --help
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_shadow_v2_manifest_audit.py
git diff --check
git diff --cached --name-only
forbidden staged-file guard
```

Runtime boundary:

```text
NO_BUY_REJECT_CHANGE
NO_GATEKEEPER_POLICY_CHANGE
NO_SELECTOR_RUNTIME_CHANGE
NO_TX_JITO_LIVE_PATH_CHANGE
NO_SHADOW_CLOSE_ONLY_ENABLEMENT
NO_ACTIVE_CLOSE_ENABLEMENT
NO_RUN_STARTED
NO_R51_TOUCH
```

Uwaga: globalna instrukcja wskazuje szablon
`docs/ADR/ADR_8D_SZABLON.md`, ale taki plik nie istnieje w aktualnym checkoutcie.
ADR zostal dopasowany do istniejacego formatu `ADR-8D` uzywanego przez Shadow
V2 w tym repo.
