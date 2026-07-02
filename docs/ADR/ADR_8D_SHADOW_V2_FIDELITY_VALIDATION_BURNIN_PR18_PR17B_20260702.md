# ADR-8D: Shadow V2 Fidelity Validation Burnin PR18 / PR17B

## Status

Accepted as report-only blocked validation evidence.

Final verdict:

`BLOCKED_POSTBUY_RUNTIME_SHUTDOWN_ABORT`

## D1. Problem

Po merge PR #25 nalezalo uruchomic PR18/PR17B validation burnin, ktory po raz pierwszy mial emitowac canonical Shadow V2:

- `shadow_entry_attempt_v2`;
- `shadow_entry_fill_v2`;
- `shadow_path_sample_v2`;
- `shadow_exit_attempt_v2`;
- `shadow_exit_fill_v2`;
- `shadow_terminal_truth_v2`.

Run nie mial byc strategy proof, edge proof, runtime approval ani live-equivalence proof.

## D2. Decyzja

Akceptujemy wynik jako:

`BLOCKED_POSTBUY_RUNTIME_SHUTDOWN_ABORT`

Nie przyznajemy:

- `runtime_approval`;
- `shadow_close_only_approval`;
- `active_close_approval`;
- `research_grade`;
- `live_equivalence`;
- strategy research unblocked.

## D3. Kontekst

Baseline main:

`e4eb37ea4b84b7c062543beae742af3e056cefc5`

Canary:

- run id: `shadow-burnin-v2-fidelity-validation-pr18-pr17b-canary-r1`;
- result: `PASS`.

Wlasciwy burnin:

- run id: `shadow-burnin-v2-fidelity-validation-pr18-pr17b-burnin-r2`;
- scope: `reports/selector/shadow-v2-fidelity-validation-pr18-pr17b-burnin-r2`;
- elapsed runtime: okolo `6h13m31s`;
- shutdown: manual SIGINT.

## D4. Evidence

Canary PASS:

- `real_shadow_v2_positions=6`;
- `ENTRY_ATTEMPT=6`;
- `ENTRY_FILL=6`;
- `PATH_SAMPLE=12`;
- `EXIT_ATTEMPT=6`;
- `EXIT_FILL=6`;
- `TERMINAL_TRUTH=6`;
- post-run strict audit: `PASS`;
- clean shutdown: `PASS`.

Burnin canonical evidence:

- `shadow_position_event_v2.jsonl`: `3055` rows;
- `shadow_replay_v2.jsonl`: `3055` rows;
- `shadow_lifecycle_v2.jsonl`: `3055` rows;
- `shadow_path_density_v2.jsonl`: `21385` rows;
- `real_shadow_v2_positions=385`;
- malformed canonical rows: `0`;
- `ENTRY_FILL:BLOCKED_BY_DATA=379`;
- `EXIT_FILL:BLOCKED_BY_DATA=381`;
- `TERMINAL_TRUTH=384`.

Offline manifest after manual generation:

- `post_run_manifest.status=PASS`;
- blockers: `[]`;
- schema coverage:
  - `shadow_position_event_v2=3055`;
  - `shadow_replay_v2=3055`;
  - `shadow_lifecycle_v2=3055`;
  - `shadow_path_density_v2=21385`.

Shutdown evidence:

- controller: `BURNIN_EXITED_BEFORE_TIMEOUT exit_code=0`;
- launcher: `Ghost Launcher shutdown complete`;
- blocker: `PostBuyRuntime shutdown join timed out after 30s; aborting task`;
- blocker: `Component shutdown completed with 1 failure(s) or forced abort(s)`.

## D5. Root Cause Classification

Root cause:

`POSTBUY_RUNTIME_SHUTDOWN_ABORTED_BEFORE_RUNTIME_POST_RUN_MANIFEST`

PostBuyRuntime zostal abortowany podczas shutdownu. W efekcie runtime nie zdazyl zapisac post-run manifestu, mimo ze canonical and derived Shadow V2 artifacts zostaly zapisane i offline manifest audit przeszedl.

## D6. Consequences

Udowodnione:

- PR18/PR17B canonical event emission dziala;
- real Shadow V2 positions sa obecne;
- V2 entry/path/exit/terminal event families sa obecne;
- replay/lifecycle/density artifacts sa generowane;
- artifact parsing i offline manifest strict audit przechodza.

Nieudowodnione:

- full clean shutdown under long burnin load;
- runtime-generated post-run manifest reliability;
- research-grade validation closure;
- live-equivalence;
- executable fill truth.

## D7. Runtime Boundary

Nie zmieniono:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close.

R51 nie byl dotykany.

Raw JSONL/log/runtime artifacts nie sa commitowane.

## D8. Required Follow-Up

Wymagany jest waski PR shutdown/manifest fix:

1. PostBuyRuntime musi miec gwarantowany Shadow V2 manifest flush before abort;
2. launcher powinien rozroznic normalny drain od forced abort;
3. long burnin musi powtorzyc PR18/PR17B evidence;
4. wymagany wynik kolejnego burnina:
   - runtime `post_run_manifest.status=PASS`;
   - post-run strict audit `PASS`;
   - no forced component abort;
   - canonical V2 fill/path/terminal rows present;
   - replay/lifecycle terminal reconciliation possible.

Do tego czasu:

- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `strategy_research_unblocked=false`;
- `live_equivalence=NOT_GRANTED`.
