# RAPORT SHADOW V2 FIDELITY VALIDATION BURNIN PR18 / PR17B 20260702

## 1. Werdykt wykonawczy

Werdykt runu:

`BLOCKED_POSTBUY_RUNTIME_SHUTDOWN_ABORT`

Run `shadow-burnin-v2-fidelity-validation-pr18-pr17b-burnin-r2` wygenerowal realne canonical Shadow V2 evidence dla fill/path/terminal, ale nie moze byc oznaczony jako pelny `PASS`, poniewaz zamkniecie procesu nie bylo w pelni clean:

- launcher wyszedl z kodem `0`;
- controller zapisal `BURNIN_EXITED_BEFORE_TIMEOUT exit_code=0`;
- `Ghost Launcher shutdown complete` wystapilo;
- `SIGTERM` nie zostal uzyty;
- ale `PostBuyRuntime shutdown join timed out after 30s; aborting task`;
- launcher zapisal `Component shutdown completed with 1 failure(s) or forced abort(s)`;
- runtime post-run manifest nie powstal przed abortem PostBuyRuntime.

Po fakcie wygenerowano offline `post_run_manifest.json` i `shadow_v2_manifest_report.csv`; strict audit offline przeszedl z `PASS`. To potwierdza komplet artefaktow evidence, ale nie kasuje blockera clean-shutdown.

## 2. Zakres i granice

Zakres wykonany:

- wykonano 15-min canary PR18/PR17B;
- po canary PASS uruchomiono wlasciwy burnin:
  `shadow-burnin-v2-fidelity-validation-pr18-pr17b-burnin-r2`;
- run byl validation/fidelity-only;
- uzyto lokalnego relaxed validation sampling configu;
- nie zmieniono kodu runtime;
- nie zmieniono produkcyjnych rolloutow.

Granice nienaruszone:

- brak strategy proof;
- brak RCE proof;
- brak selector/edge proof;
- brak runtime approval;
- brak live-equivalence claim;
- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- brak zmian BUY/REJECT;
- brak zmian Gatekeeper policy;
- brak zmian selector runtime;
- brak zmian TX/Jito/live path;
- brak `shadow_close_only`;
- brak active close;
- brak R51;
- raw JSONL/log/runtime artifacts nie sa commitowane.

## 3. Baseline i config

Baseline main po merge PR #25:

`e4eb37ea4b84b7c062543beae742af3e056cefc5`

Canary run:

- run id: `shadow-burnin-v2-fidelity-validation-pr18-pr17b-canary-r1`;
- scope: `reports/selector/shadow-v2-fidelity-validation-pr18-pr17b-canary-r1`.

Wlasciwy burnin:

- run id: `shadow-burnin-v2-fidelity-validation-pr18-pr17b-burnin-r2`;
- scope: `reports/selector/shadow-v2-fidelity-validation-pr18-pr17b-burnin-r2`;
- local launcher config:
  `configs/rollout/shadow-v2-fidelity-validation-pr18-pr17b-burnin-r2.local.toml`;
- local Ghost Brain config:
  `configs/rollout/ghost_brain_shadow_v2_fidelity_validation_pr18_pr17b_burnin_r2.local.toml`.

Preflight potwierdzil:

- execution mode: `Shadow`;
- entry mode: `shadow_only`;
- Gatekeeper V2: `min_tx=10`, `min_unique=6`, `min_buy=5`, `max_wait_ms=10000`;
- Spectrum RPC: `PASS`;
- NLN gRPC app probe: `PASS`;
- trigger balance preflight: `PASS`;
- metrics port: `PASS`.

## 4. Canary result

Canary PR18/PR17B:

- start: `2026-07-02T02:05:29Z`;
- shutdown: SIGINT po 15 min;
- launcher exit code: `0`;
- post-run strict audit: `PASS`;
- `SIGTERM`: `0`;
- `Transport channel disconnected`: `0`;
- `All components shut down successfully`: true;
- `Ghost Launcher shutdown complete`: true.

Canary canonical V2 evidence:

- `POSITION_CREATED`: `7`;
- `ENTRY_ATTEMPT`: `6`;
- `ENTRY_FILL`: `6`;
- `PATH_SAMPLE`: `12`;
- `EXIT_ATTEMPT`: `6`;
- `EXIT_FILL`: `6`;
- `TERMINAL_TRUTH`: `6`;
- `real_shadow_v2_positions`: `6`;
- `ENTRY_FILL:BLOCKED_BY_DATA`: `6`;
- `EXIT_FILL:BLOCKED_BY_DATA`: `6`;
- malformed rows: `0`.

Canary dal zielone swiatlo do wlasciwego burnina.

## 5. Burnin execution

Pierwsza proba `burnin-r1` zostala odrzucona jako niewazna operacyjnie: proces zginal po odpieciu sesji narzedzia bez clean shutdown i bez runtime error. Nie jest traktowana jako wynik walidacji.

Wlasciwy run:

- run id: `shadow-burnin-v2-fidelity-validation-pr18-pr17b-burnin-r2`;
- controller start: `2026-07-02T02:24:29Z`;
- manual SIGINT: okolo `2026-07-02T08:37:26Z`;
- launcher shutdown complete: `2026-07-02T08:37:36.709988Z`;
- controller exit: `2026-07-02T08:38:00Z`;
- elapsed runtime: okolo `6h13m31s`;
- controller status: `BURNIN_EXITED_BEFORE_TIMEOUT exit_code=0`;
- hard timeout 8h nie zostal osiagniety.

## 6. Canonical V2 evidence

`shadow_position_event_v2.jsonl`:

- total rows: `3055`;
- malformed rows: `0`;
- `POSITION_CREATED`: `386`;
- `ENTRY_ATTEMPT`: `379`;
- `ENTRY_FILL`: `379`;
- `PATH_SAMPLE`: `765`;
- `EXIT_ATTEMPT`: `381`;
- `EXIT_FILL`: `381`;
- `TERMINAL_TRUTH`: `384`;
- `real_shadow_v2_positions`: `385`;
- `ENTRY_FILL:BLOCKED_BY_DATA`: `379`;
- `EXIT_FILL:BLOCKED_BY_DATA`: `381`.

Derived artifacts:

- `shadow_replay_v2.jsonl`: `3055` rows;
- `shadow_lifecycle_v2.jsonl`: `3055` rows;
- `shadow_path_density_v2.jsonl`: `21385` rows.

Offline manifest after manual generation:

- `post_run_manifest.status=PASS`;
- blockers: `[]`;
- artifact_count: `10`;
- schema coverage:
  - `shadow_position_event_v2`: `3055`;
  - `shadow_replay_v2`: `3055`;
  - `shadow_lifecycle_v2`: `3055`;
  - `shadow_path_density_v2`: `21385`;
- total_size_bytes: `3182138070`.

## 7. Shutdown blocker

Typed blocker:

`POSTBUY_RUNTIME_SHUTDOWN_ABORTED_BEFORE_RUNTIME_POST_RUN_MANIFEST`

Evidence:

- `PostBuyRuntime shutdown join timed out after 30s; aborting task`;
- `Component shutdown completed with 1 failure(s) or forced abort(s)`;
- runtime did not produce `post_run_manifest.json` before the abort;
- offline manifest generation later succeeded.

Interpretacja:

- canonical V2 evidence exists and is parseable;
- derived replay/lifecycle/density evidence exists;
- offline manifest audit passes;
- clean shutdown gate is not satisfied because PostBuyRuntime was aborted during shutdown.

## 8. Co zostalo udowodnione

Udowodnione:

- PR18/PR17B runtime potrafi emitowac canonical `shadow_entry_attempt_v2`;
- runtime potrafi emitowac canonical `shadow_entry_fill_v2`;
- runtime potrafi emitowac canonical `shadow_path_sample_v2`;
- runtime potrafi emitowac canonical `shadow_exit_attempt_v2`;
- runtime potrafi emitowac canonical `shadow_exit_fill_v2`;
- runtime potrafi emitowac canonical `shadow_terminal_truth_v2`;
- replay/lifecycle/density rows sa generowane z canonical stream;
- `real_shadow_v2_positions > 0`;
- malformed canonical rows = `0`;
- entry/exit fill statusy sa typed `BLOCKED_BY_DATA` tam, gdzie fill data nie wystarcza.

## 9. Co pozostaje zablokowane

Zablokowane:

- pelny burnin PASS;
- research-grade verdict;
- runtime approval;
- shadow_close_only approval;
- active close approval;
- live-equivalence claim;
- PR17 fidelity validation as final PASS;
- automatic post-run manifest reliability claim.

Bez kolejnej poprawki shutdown PostBuyRuntime nie wolno traktowac tego runu jako pelnego closure evidence, mimo ze dane V2 sa bogate.

## 10. Decyzja

Ten PR jest report-only i zapisuje stan:

`BLOCKED_POSTBUY_RUNTIME_SHUTDOWN_ABORT`

Nastepny krok powinien byc waski PR naprawiajacy shutdown PostBuyRuntime / manifest flush path:

1. PostBuyRuntime musi flushowac Shadow V2 harness i manifest przed bounded join abort;
2. albo launcher musi miec osobny graceful drain budget dla Shadow V2 post-run manifest;
3. po fixie nalezy powtorzyc PR18/PR17B burnin;
4. dopiero wtedy mozna rozstrzygac `PASS` vs `FAIL` dla fidelity validation.

Do tego czasu:

- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `strategy_research_unblocked=false`;
- `live_equivalence=NOT_GRANTED`.
