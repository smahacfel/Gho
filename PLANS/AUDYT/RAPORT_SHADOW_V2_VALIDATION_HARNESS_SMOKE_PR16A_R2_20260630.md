# RAPORT SHADOW V2 VALIDATION HARNESS SMOKE PR16A R2 20260630

## 1. Werdykt wykonawczy

Werdykt smoke r2:

`FAIL_BLOCKED_SCHEMA_CONTRACT_AND_SHUTDOWN`

PR16A naprawił krytyczny blocker z poprzedniego smoke: harness nie zależy już od losowego BUY ani `PostBuySubmitted`, ponieważ deterministic smoke marker został zapisany przez realny `ShadowV2ValidationHarness::append_record()`.

Smoke r2 nie spełnia jednak bramki pozytywnej, ponieważ:

- `shadow_position_event_v2.jsonl` ma `rows=1`;
- `shadow_replay_v2.jsonl` ma `rows=1`;
- `shadow_lifecycle_v2.jsonl` ma `rows=1`;
- `shadow_path_density_v2.jsonl` ma `rows=7`;
- `post_run_manifest.json` istnieje, ale ma `status=BLOCKED`;
- post-run strict audit kończy się `FAIL`;
- clean shutdown nie został udowodniony: runtime przyjął shutdown signal, ale nie zakończył się po dwóch SIGINT; proces został zatrzymany dopiero przez SIGTERM.

To nie jest research-grade burnin i nie jest PR17 fidelity validation burnin.

## 2. Zakres operacyjny

Zakres wykonany:

- PR #13 był już merged do `main`;
- lokalny `main` został zaktualizowany do `ae88251fc582b15275f337552535a4d783d9fd56`;
- wygenerowano świeży `pre_run_manifest.json` dla `shadow-burnin-v2-fidelity-validation-logging-only-smoke-r2`;
- uruchomiono wyłącznie logging-only smoke z `shadow_v2_burnin.enabled=true` oraz `logging_only=true`;
- nie uruchomiono PR17 fidelity validation burnin;
- nie uruchomiono RCE proof;
- nie zmieniono BUY/REJECT, Gatekeeper policy, selector runtime ani TX/Jito/live path.

## 3. Backup i scope

Backup poprzedniego PR16 smoke istnieje nadal:

- `/tmp/shadow-v2-fidelity-validation-pr16-backup-20260630T234254Z`

Ponieważ `/tmp` nie jest trwałym miejscem backupu, wykonano dodatkową kopię poza repo:

- `/root/Gho_shadow_v2_backups/shadow-v2-fidelity-validation-pr16-backup-20260630T234254Z`

Nie usunięto backupu z `/tmp`.

Scope smoke r2:

- `reports/selector/shadow-v2-fidelity-validation`

Raw evidence pozostaje lokalne i nie jest przeznaczone do commita.

## 4. Pre-run i preflight

Pre-run manifest generation:

- status: `PASS`
- blockers: `[]`
- run_id: `shadow-burnin-v2-fidelity-validation-logging-only-smoke-r2`

Pre-run strict audit:

- status: `PASS`
- blockers: `[]`

Launcher preflight:

- status: `PASS`
- NLN gRPC app probe: `PASS`
- RPC getVersion: `PASS`
- trigger balance: `PASS`

Nie stwierdzono problemu z autoryzacją NLN API key ani z samym zestawieniem streamu. Stream później został zestawiony w runtime.

## 5. Runtime smoke evidence

Runtime uruchomił Shadow V2 logging-only harness i wyemitował deterministic smoke marker.

Dowody z logu:

- `PostBuyRuntime: Shadow V2 validation smoke marker emitted`
- `Stream established`
- `PostBuyRuntime received shutdown signal; draining late PostBuySubmitted events for 10000ms`

Rzeczywiste artefakty Shadow V2:

| Artifact | Rows | Status |
|---|---:|---|
| `shadow_position_event_v2.jsonl` | 1 | PRESENT |
| `shadow_replay_v2.jsonl` | 1 | PRESENT |
| `shadow_lifecycle_v2.jsonl` | 1 | PRESENT |
| `shadow_path_density_v2.jsonl` | 7 | PRESENT |
| `post_run_manifest.json` | n/a | PRESENT_BUT_BLOCKED |
| `shadow_v2_manifest_report.csv` | n/a | PRESENT |

To potwierdza, że PR16A rozwiązał poprzedni blocker `FAIL_BLOCKED_NO_CANONICAL_V2_EVIDENCE`.

## 6. Post-run blocker

Post-run manifest:

- status: `BLOCKED`
- blockers:
  - `shadow_position_event_v2.jsonl: expected schema shadow_position_event_v2 not found`

Post-run strict audit:

- status: `FAIL`
- blocker:
  - `shadow_position_event_v2.jsonl: expected schema shadow_position_event_v2 not found`

Rzeczywisty pierwszy rekord `shadow_position_event_v2.jsonl` ma:

- top-level `schema`: missing;
- `envelope.schema`: `shadow_position_v2`;
- `event_kind`: present;
- `payload`: present.

Audyt manifestu oczekuje schemy `shadow_position_event_v2`, ale canonical writer zapisuje envelope/payload dla `shadow_position_v2` w pliku `shadow_position_event_v2.jsonl`. To jest kontraktowy mismatch między wrapperem canonical-event artifact a schema coverage audit.

## 7. Clean shutdown blocker

Shutdown:

- pierwszy SIGINT: proces przyjął shutdown signal, ale nie zakończył się;
- drugi SIGINT: proces nadal nie zakończył się;
- runtime kontynuował logowanie powtarzających się `Transport channel disconnected`;
- proces `ghost-launcher` został zatrzymany dopiero przez SIGTERM;
- exit code z sesji: `1`.

Wniosek:

`clean_shutdown_proven=false`

Smoke r2 nie może dostać PASS, nawet mimo tego, że writer/materializer wygenerował wymagane JSONL rows.

## 8. NLN / gRPC observations

NLN/gRPC:

- preflight NLN app probe: `PASS`;
- runtime stream: `Stream established`;
- po rozpoczęciu shutdownu pojawił się flood `Transport channel disconnected`.

Ten raport nie klasyfikuje tego jako problemu z endpointem lub API key, ponieważ stream został poprawnie zestawiony i przetwarzał eventy. Problem jest obserwowany na granicy shutdown/transport close loop i wymaga osobnej analizy PR16B/PR15-fix, a nie podmiany adresów NLN na tym etapie.

## 9. Guard rails

Status approval:

- `runtime_approval=false`
- `shadow_close_only_approval=false`
- `active_close_approval=false`
- `strategy_research_unblocked=false`
- `research_grade=NOT_GRANTED`
- `live_equivalence=NOT_GRANTED`
- `PR17 fidelity validation burnin=BLOCKED`

Nie wykonano:

- PR17 full fidelity validation burnin;
- RCE proof;
- strategy proof;
- selector proof;
- edge proof;
- runtime approval;
- shadow_close_only approval;
- active close approval.

## 10. Następne wymagane poprawki

Przed PR17 potrzebny jest mały fix PR16B albo PR15-fix:

1. Ujednolicić kontrakt `shadow_position_event_v2.jsonl`:
   - albo canonical wrapper ma emitować top-level schema `shadow_position_event_v2`;
   - albo manifest audit musi świadomie akceptować canonical wrapper z `envelope.schema=shadow_position_v2` jako artifact `shadow_position_event_v2`;
   - wybór musi być zapisany w schema manifest i testach.

2. Naprawić clean shutdown path:
   - SIGINT musi kończyć logging-only smoke bez SIGTERM;
   - post-run manifest generation i strict verification muszą domknąć się przed wyjściem;
   - transport disconnect loop nie może blokować zakończenia procesu.

3. Powtórzyć smoke:
   - `shadow_position_event_v2 rows > 0`;
   - `shadow_replay_v2 rows > 0`;
   - `shadow_lifecycle_v2 rows > 0`;
   - `shadow_path_density_v2 rows > 0`;
   - `post_run_manifest.status=PASS`;
   - `post_run_strict_audit=PASS`;
   - `clean_shutdown_proven=true`.

## 11. Decyzja

PR16A smoke r2 jest wartościowym negatywnym dowodem operacyjnym:

- canonical writer/materializer path działa na tyle, że generuje wymagane JSONL rows bez BUY/handoff;
- manifest strict gate nadal blokuje przez schema mismatch;
- clean shutdown nadal nie jest udowodniony.

Final:

`PR17_REMAINS_BLOCKED`
