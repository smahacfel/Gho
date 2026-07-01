# RAPORT SHADOW V2 VALIDATION HARNESS SMOKE PR16B R3 20260701

## 1. Werdykt wykonawczy

Werdykt smoke r3 po merge PR16B:

`FAIL_BLOCKED_CLEAN_SHUTDOWN_SEER_TRANSPORT_LOOP`

PR16B naprawił dwa poprzednie blokery:

- `shadow_position_event_v2.jsonl` ma top-level `schema=shadow_position_event_v2`;
- `post_run_manifest.json` ma `status=PASS`;
- post-run strict audit ma `PASS`;
- OracleRuntime kończy się po shutdown i loguje `Oracle Runtime shut down successfully`.

Smoke r3 nadal nie spełnia pełnej bramki pozytywnej, ponieważ proces `ghost-launcher` nie zakończył się po pierwszym ani drugim SIGINT. Po shutdown signal komponent Seer wszedł w powtarzający się loop `Transport channel disconnected`; proces został zatrzymany dopiero przez SIGTERM.

To nie jest PR17 fidelity validation burnin i nie jest research-grade evidence.

## 2. Zakres operacyjny

Zakres wykonany:

- lokalny `main` został zaktualizowany do merge commit PR16B:
  `29032341089a28217035cc6f6d56594788aa02c7`;
- przygotowano świeży scope:
  `reports/selector/shadow-v2-fidelity-validation`;
- poprzedni scope smoke został przeniesiony bez kasowania do trwałego backupu:
  `/root/Gho_shadow_v2_backups/shadow-v2-fidelity-validation-before-r3-20260701T031717Z`;
- wygenerowano świeży `pre_run_manifest.json` dla:
  `shadow-burnin-v2-fidelity-validation-logging-only-smoke-r3`;
- uruchomiono wyłącznie logging-only smoke z `shadow_v2_burnin.enabled=true` i `logging_only=true`;
- nie uruchomiono PR17 fidelity validation burnin.

Zakres wykluczony:

- brak RCE proof;
- brak strategy proof;
- brak selector proof;
- brak edge proof;
- brak zmian BUY/REJECT;
- brak zmian Gatekeeper policy;
- brak zmian selector runtime;
- brak zmian TX/Jito/live path;
- brak `shadow_close_only`;
- brak active close;
- brak ingerencji w R51.

## 3. Pre-run i preflight

Pre-run manifest generation:

- status: `PASS`;
- blockers: `[]`;
- run_id: `shadow-burnin-v2-fidelity-validation-logging-only-smoke-r3`.

Pre-run strict audit:

- status: `PASS`;
- blockers: `[]`.

Launcher preflight:

- status: `PASS`;
- execution mode: `Shadow`;
- entry mode: `shadow_only`;
- `seer.grpc_endpoint` app probe: `PASS`;
- trigger RPC getVersion: `PASS`;
- trigger balance: `PASS`;
- metrics port: `PASS`.

Nie stwierdzono problemu z NLN auth ani endpointem:

- `PermissionDenied`: `0`;
- `Account disabled`: `0`;
- runtime stream: `Stream established`.

## 4. Runtime evidence

Runtime smoke potwierdził:

- `All components started`: `1`;
- `Stream established`: `1`;
- `PostBuyRuntime: Shadow V2 validation smoke marker emitted`: `1`;
- `Oracle Runtime shut down successfully`: `1`;
- `PostBuyRuntime received shutdown signal`: `1`;
- `Seer: Shutdown signal received`: `1`.

Rzeczywiste artefakty Shadow V2:

| Artifact | Rows | Status |
|---|---:|---|
| `shadow_position_event_v2.jsonl` | 1 | PASS |
| `shadow_replay_v2.jsonl` | 1 | PASS |
| `shadow_lifecycle_v2.jsonl` | 1 | PASS |
| `shadow_path_density_v2.jsonl` | 7 | PASS |
| `post_run_manifest.json` | n/a | PASS |
| `shadow_v2_manifest_report.csv` | n/a | PRESENT |

Schema check dla pierwszego canonical record:

- top-level `schema`: `shadow_position_event_v2`;
- `canonical_payload_schema`: `shadow_position_v2`;
- `event_kind`: `POSITION_CREATED`.

To potwierdza, że PR16B zamknął poprzedni schema contract blocker.

## 5. Post-run manifest

Post-run manifest:

- status: `PASS`;
- blockers: `[]`;
- schema coverage:
  - `shadow_position_event_v2`: `1`;
  - `shadow_replay_v2`: `1`;
  - `shadow_lifecycle_v2`: `1`;
  - `shadow_path_density_v2`: `7`.

Post-run strict audit:

- status: `PASS`;
- blockers: `[]`.

Plan/downgrade static guards:

- `python3 scripts/shadow_v2_validation_burnin_plan_audit.py --strict`: `PASS`;
- `python3 scripts/shadow_v2_legacy_downgrade_audit.py --strict`: `PASS`.

## 6. Clean shutdown blocker

Clean shutdown nie został udowodniony.

Chronologia:

- `03:18:48.432Z`: `Shutdown signal received, stopping all components...`;
- `03:18:48.432Z`: `Waiting for Oracle Runtime to shut down...`;
- `03:18:48.433Z`: `PostBuyRuntime received shutdown signal`;
- `03:18:48.433Z`: `Oracle Runtime shut down successfully`;
- `03:18:48.435Z`: `LivePipelineFlushLoop: Shutdown signal received`;
- `03:18:48.435Z`: `Seer: Shutdown signal received`;
- po tym runtime kontynuował flood `Transport channel disconnected`.

Liczba `Transport channel disconnected` w logu smoke r3:

`29034`

Zachowanie procesu:

- pierwszy SIGINT: proces nie zakończył się po 30 sekundach;
- drugi SIGINT: proces nie zakończył się po kolejnych 30 sekundach;
- proces zakończono SIGTERM;
- `clean_shutdown_proven=false`;
- `forced_stop_used=SIGTERM`.

Wniosek:

PR16B naprawił OracleRuntime shutdown, ale nie domknął lifecycle całego procesu. Obecnym blockerem jest Seer/gRPC transport loop po shutdown.

## 7. Klasyfikacja problemu NLN/gRPC

To nie wygląda na błąd autoryzacji NLN ani niedziałający endpoint:

- preflight NLN app probe przeszedł;
- runtime zestawił stream;
- brak `PermissionDenied`;
- brak `Account disabled`.

Problem jest obserwowany dopiero po shutdown signal i dotyczy zachowania transportu/loopu zamykania Seer/gRPC.

## 8. Guard rails

Status approval pozostaje:

- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `strategy_research_unblocked=false`;
- `research_grade=NOT_GRANTED`;
- `live_equivalence=NOT_GRANTED`;
- `PR17 fidelity validation burnin=BLOCKED`.

Raw evidence:

- istnieje lokalnie w `reports/selector/shadow-v2-fidelity-validation`;
- nie jest przeznaczone do commita;
- nie powinno być stage'owane.

## 9. Następny wymagany krok

Przed PR17 potrzebny jest PR16C / PR15-fix dla shutdownu Seer/gRPC:

1. Seer/gRPC musi przestać retry/flood po globalnym shutdown signal.
2. Shutdown receiver musi przerwać transport reconnect/receive loop.
3. Proces musi wyjść po pierwszym SIGINT bez SIGTERM.
4. Post-run manifest generation i strict audit muszą pozostać PASS.

Po tej poprawce trzeba powtórzyć smoke i wymagać:

- `shadow_position_event_v2 rows > 0`;
- `shadow_replay_v2 rows > 0`;
- `shadow_lifecycle_v2 rows > 0`;
- `shadow_path_density_v2 rows > 0`;
- `post_run_manifest.status=PASS`;
- `post_run_strict_audit=PASS`;
- `clean_shutdown_proven=true`.

## 10. Decyzja

Smoke r3 jest wartościowym negatywnym dowodem:

- writer/materializer/manifest path działa;
- schema blocker został naprawiony;
- OracleRuntime shutdown blocker został naprawiony;
- globalny clean shutdown procesu nadal failuje przez Seer/gRPC shutdown loop.

Final:

`PR17_REMAINS_BLOCKED`
