# RAPORT SHADOW V2 FIDELITY VALIDATION BURNIN PR17 20260701

## 1. Werdykt wykonawczy

Werdykt PR17 fidelity validation burnin:

`BLOCKED_NO_REAL_SHADOW_V2_POSITION_EVIDENCE`

To nie jest awaria infrastruktury Shadow V2 harness. Aktualny run potwierdzil, ze sciezka:

`pre_run_manifest -> launcher preflight -> runtime -> canonical JSONL -> derived replay/lifecycle -> density rows -> post_run_manifest PASS -> strict audit PASS -> clean shutdown`

dziala poprawnie w trybie `shadow_v2_burnin.enabled=true` i `shadow_v2_burnin.logging_only=true`.

Blokada dotyczy fidelity validation: w aktualnym oknie nie powstala zadna realna Shadow V2 pozycja z entry/fill/path/exit. Jedyne rekordy Shadow V2 pochodza z diagnostycznego markera harnessu `VALIDATION_SMOKE_MARKER`, ktory jest oznaczony jako `DIAGNOSTIC_ONLY`, `UNKNOWN` i `BLOCKED_BY_DATA`.

Dlatego:

- harness/materializer/manifest/shutdown: `PASS`;
- realna walidacja entry price: `BLOCKED_NO_REAL_ENTRY_FILL`;
- realna walidacja exit price: `BLOCKED_NO_REAL_EXIT_FILL`;
- realna walidacja replay/lifecycle dla pozycji: `BLOCKED_MARKER_ONLY`;
- temporal/no-lookahead dla realnych pol: `BLOCKED_NO_REAL_POSITION_FIELDS`;
- reconstruction readiness: `BLOCKED_NO_REAL_SHADOW_V2_POSITION_EVIDENCE`;
- research-grade: `NOT_GRANTED`;
- live-equivalence: `NOT_GRANTED`.

## 2. Zakres i granice

PR17 zostal wykonany jako `validation/fidelity-only`.

Zakres wykluczony:

- brak strategy proof;
- brak runtime approval;
- brak `shadow_close_only`;
- brak active close;
- brak live-equivalence claim;
- brak zmian BUY/REJECT;
- brak zmian Gatekeeper policy;
- brak zmian selector runtime;
- brak zmian TX/Jito/live path;
- brak RCE proof;
- brak R51.

Approval flags pozostaja:

- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `strategy_research_unblocked=false`.

## 3. Baseline i konfiguracja

Baseline main:

`286a1f76f5c5fd632800f60afa6b4be98066eec7`

Wymaganie `main >= 286a1f76f5c5fd632800f60afa6b4be98066eec7` zostalo spelnione.

Run id:

`shadow-burnin-v2-fidelity-validation-pr17-r1`

Scope root:

`reports/selector/shadow-v2-fidelity-validation-pr17-r1`

Do uruchomienia uzyto lokalnych, niestage'owanych configow `*.local.toml`:

- `configs/rollout/shadow-v2-fidelity-validation-pr17-r1.local.toml`;
- `configs/rollout/ghost_brain_shadow_v2_fidelity_validation_pr17_r1.local.toml`.

Konfiguracja runtime zachowala:

- `execution_mode = "shadow"`;
- `entry_mode = "shadow_only"`;
- `shadow_v2_burnin.enabled=true`;
- `shadow_v2_burnin.logging_only=true`;
- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- NLN gRPC jako glowny ingest: `grpc.nln.clr3.org:443`;
- NLN Program Streams endpoint: `events.nln.clr3.org:443`;
- Program Streams topics:
  - `solana.pump_fun.buy`;
  - `solana.pump_fun.buy_exact_sol_in`;
- Spectrum jako RPC endpoint dla shadow burnin validation.

Sekrety i lokalne overlay configi nie sa czescia tego PR.

## 4. Pre-run gates

Pre-run manifest generation:

- status: `PASS`;
- blockers: `[]`;
- run_id: `shadow-burnin-v2-fidelity-validation-pr17-r1`;
- created_at: `2026-07-01T23:18:09+00:00`.

Pre-run strict manifest audit:

- status: `PASS`;
- blockers: `[]`.

Validation burnin plan audit:

- command: `python3 scripts/shadow_v2_validation_burnin_plan_audit.py --strict`;
- status: `PASS`;
- `validation_mode=FIDELITY_ONLY`;
- `plan_status=PLAN_ONLY`;
- `runtime_approval=false`;
- `strategy_proof_enabled=false`.

Legacy downgrade audit:

- command: `python3 scripts/shadow_v2_legacy_downgrade_audit.py --strict`;
- status: `PASS`;
- `v1_live_equivalent_allowed=false`.

Build i runtime preflight:

- `cargo build -p ghost-launcher --bin ghost-launcher --release`: `PASS`;
- launcher `--preflight`: `PASS`;
- execution mode: `Shadow`;
- entry mode: `shadow_only`;
- Spectrum RPC preflight: `PASS`;
- NLN primary gRPC app probe: `PASS`.

## 5. Runtime evidence

Run start:

`2026-07-01T23:18:35.652Z`

Shutdown signal:

`2026-07-01T23:24:38.679Z`

Final shutdown:

`2026-07-01T23:24:48.840Z`

Runtime observations:

- `All components started successfully`: `1`;
- NLN main gRPC stream established: `PASS`;
- Program Streams started for both required topics: `PASS`;
- `Shadow V2 validation smoke marker emitted`: `1`;
- `NewPoolDetected`: `420`;
- `Detected new pool`: `210`;
- `Runtime Shadow Buy Submitted`: `0`;
- `NLN Subscribe request failed`: `0`;
- `Transport channel disconnected`: `0`;
- `status: 502`: `0`;
- `Bad Gateway`: `0`.

Decision logs, stored under local `logs/rollout/...` and not committed:

- gatekeeper decision files: `2`;
- gatekeeper decision rows: `286`;
- malformed decision rows: `0`;
- `decision_verdict_buy=true`: `0`;
- `decision_verdict_buy=false_or_missing`: `286`;
- decision planes:
  - `legacy_live`: `210`;
  - `v25_shadow`: `76`;
- selector shadow score rows: `286`.

Te decision logs sa runtime artifacts i nie sa czescia commita PR17.

## 6. Shadow V2 artifacts

Rzeczywiste artefakty Shadow V2 w scope:

| Artifact | Rows | Status | Uwagi |
|---|---:|---|---|
| `shadow_position_event_v2.jsonl` | 1 | PASS_INFRA | top-level schema `shadow_position_event_v2` |
| `shadow_replay_v2.jsonl` | 1 | PASS_INFRA | derived snapshot z canonical high-watermark |
| `shadow_lifecycle_v2.jsonl` | 1 | PASS_INFRA | derived snapshot z canonical high-watermark |
| `shadow_path_density_v2.jsonl` | 7 | PASS_INFRA | wszystkie verdicts `NOT_EVALUABLE_NO_COVERAGE` |
| `post_run_manifest.json` | n/a | PASS | blockers `[]` |

Canonical row nie jest realna pozycja strategii. To marker:

- `candidate_id=VALIDATION_SMOKE_MARKER`;
- `pool_id=VALIDATION_SMOKE_POOL_UNKNOWN`;
- `base_mint=VALIDATION_SMOKE_BASE_MINT_UNKNOWN`;
- `simulation_level=MARK_ONLY`;
- `measurement_grade=DIAGNOSTIC_ONLY`;
- `temporal_class=UNKNOWN`;
- `quality=VALIDATION_SMOKE_MARKER_BLOCKED_BY_DATA`;
- limitations zawieraja:
  - `VALIDATION_SMOKE_MARKER_V2`;
  - `DIAGNOSTIC_ONLY_NOT_STRATEGY_POSITION`;
  - `BLOCKED_BY_DATA_NO_ENTRY_FILL_EXIT_FILL_OR_PATH`;
  - `NOT_CONSUMED_BY_DECISIONS`;
  - `NOT_STRATEGY_EVIDENCE`;
  - `NOT_LIVE_EQUIVALENT`;
  - `NO_BUY_REJECT_CHANGE`.

## 7. Post-run manifest

Post-run manifest zapisany przez harness:

- status: `PASS`;
- blockers: `[]`;
- artifact_count w zapisanym manifest file: `6`;
- schema coverage:
  - `shadow_position_event_v2`: `1`;
  - `shadow_replay_v2`: `1`;
  - `shadow_lifecycle_v2`: `1`;
  - `shadow_path_density_v2`: `7`.

Niezalezny post-run strict audit:

- command: `python3 scripts/shadow_v2_manifest_audit.py --scope-root reports/selector/shadow-v2-fidelity-validation-pr17-r1 --manifest-phase post_run --schema-manifest reports/selector/shadow_v2_required_schema_manifest.csv --acceptance-gates reports/selector/shadow_v2_acceptance_gates.csv --strict`;
- status: `PASS`;
- blockers: `[]`;
- artifact_count ze strict scan: `7`;
- total_size_bytes: `21986`.

Roznica `artifact_count=6` w zapisanym manifeście vs `artifact_count=7` w strict scan wynika z tego, ze strict scan liczy rowniez juz istniejacy zapisany manifest/report przy ponownym skanie. Nie zmienia to bramki: oba przebiegi maja `status=PASS` i `blockers=[]`.

## 8. Clean shutdown

Clean shutdown zostal udowodniony:

- wyslano jeden `SIGINT`;
- `SIGTERM`: `0`;
- `Oracle Runtime shut down successfully`: `1`;
- `PostBuyRuntime: Shadow V2 post-run manifest generated and strict-verified`: `1`;
- `POST_RUN_MANIFEST_AUDIT_PASS`: `1`;
- `Seer shut down successfully`: `1`;
- `Watchdog shut down successfully`: `1`;
- `All components shut down successfully`: `1`;
- `Ghost Launcher shutdown complete`: `1`;
- process exit code: `0`.

Nie zaobserwowano reconnect/disconnect flood po shutdownie:

- `Transport channel disconnected`: `0`;
- `NLN Subscribe request failed`: `0`;
- `status: 502`: `0`;
- `Bad Gateway`: `0`.

## 9. Fidelity validation status

### Entry price fidelity

Status:

`BLOCKED_NO_REAL_ENTRY_FILL`

Powod:

- brak realnego `ShadowEntryAttemptV2`;
- brak realnego `ShadowEntryFillV2`;
- brak realnego pool-state reference dla entry;
- brak executable fill price;
- brak slippage/own-impact/fee evidence dla entry.

Nie wolno inferowac entry price fidelity z markera `VALIDATION_SMOKE_MARKER`.

### Exit price fidelity

Status:

`BLOCKED_NO_REAL_EXIT_FILL`

Powod:

- brak realnego `ShadowExitAttemptV2`;
- brak realnego `ShadowExitFillV2`;
- brak path samples;
- density verdicts sa `NOT_EVALUABLE_NO_COVERAGE`;
- brak target/stop/timeout reconstruction input.

Nie wolno inferowac exit price fidelity z derived replay/lifecycle markera.

### Replay/lifecycle consistency

Status:

`BLOCKED_MARKER_ONLY`

Powod:

- replay i lifecycle powstaly z tego samego canonical high-watermark;
- to potwierdza infrastrukture materializacji;
- nie potwierdza spójnosci dla realnej pozycji, bo canonical stream ma tylko marker diagnostyczny.

### Temporal/no-lookahead

Status:

`BLOCKED_NO_REAL_POSITION_FIELDS`

Powod:

- marker jest `temporal_class=UNKNOWN` i `DIAGNOSTIC_ONLY`;
- brak realnych fieldow entry/exit/path do klasyfikacji temporalnej;
- brak podstaw do proof no-lookahead dla realnej symulacji.

### Reconstruction readiness

Status:

`BLOCKED_NO_REAL_SHADOW_V2_POSITION_EVIDENCE`

Powod:

- brak realnych fills;
- brak path coverage;
- brak pool state samples dla realnych pozycji;
- brak executable entry/exit quotes.

## 10. Znaczenie dla strategii

PR17 r1 nie odblokowuje strategii.

Nie wolno cytowac tego runu jako dowodu:

- live PnL;
- executable fills;
- live slippage behavior;
- landing outcome;
- shadow_close_only readiness;
- active close readiness;
- RCE proof;
- selector edge.

Stare downgrade labels pozostaja aktualne:

- Shadow V1 nie jest live-equivalent;
- stare raporty nie sa dowodem live PnL ani executable fills;
- R51 pozostaje poza zakresem PR17;
- dalsza strategia pozostaje zablokowana do czasu realnego Shadow V2 fidelity evidence.

## 11. Final decision

| Pytanie | Odpowiedz |
|---|---|
| Czy PR17 harness uruchomil sie i zamknal poprawnie? | Tak |
| Czy pre-run strict audit przeszedl? | Tak |
| Czy post-run manifest ma `PASS`? | Tak |
| Czy post-run strict audit przeszedl? | Tak |
| Czy clean shutdown jest udowodniony? | Tak |
| Czy raw JSONL/logi sa commitowane? | Nie |
| Czy powstaly realne Shadow V2 positions? | Nie |
| Czy entry prices sa proven? | Nie, `BLOCKED_NO_REAL_ENTRY_FILL` |
| Czy exit prices sa proven? | Nie, `BLOCKED_NO_REAL_EXIT_FILL` |
| Czy replay/lifecycle sa proven dla realnych pozycji? | Nie, `BLOCKED_MARKER_ONLY` |
| Czy path density jest evaluable? | Nie, `NOT_EVALUABLE_NO_COVERAGE` |
| Czy research-grade jest przyznany? | Nie |
| Czy live-equivalence jest przyznane? | Nie |
| Czy runtime approval jest przyznany? | Nie |

Finalny verdict PR17 r1:

`BLOCKED_NO_REAL_SHADOW_V2_POSITION_EVIDENCE`

## 12. Wymagany nastepny krok

Nie nalezy przechodzic do strategy proof ani runtime approval.

Kolejny krok powinien byc osobna decyzja operatora o sposobie uzyskania realnego Shadow V2 fidelity evidence bez zmiany BUY/REJECT:

1. powtorzyc validation burnin z dluzszym oknem i ta sama polityka, liczac na realny accepted shadow handoff;
2. albo zaprojektowac waski diagnostic-only producer realnych candidate/position records oznaczonych `BLOCKED_BY_DATA`, bez konsumpcji przez Gatekeeper/selector/TX;
3. albo uruchomic osobny fixture/live-capture validation mode, ktory nie jest strategy proof i nie zmienia runtime decisions.

Kazda z tych opcji wymaga osobnej akceptacji. PR17 r1 sam w sobie pozostaje `BLOCKED`, bo nie wygenerowal realnej pozycji do rekonstrukcji fidelity.
