# RAPORT SHADOW V2 VALIDATION HARNESS SMOKE PR16D R4 20260701

## 1. Werdykt wykonawczy

Werdykt smoke r4 po merge PR16C:

`FAIL_BLOCKED_LAUNCHER_WATCHDOG_RECONCILIATION_SHUTDOWN_WAIT`

PR16C naprawil blocker z r3 dotyczacy Seer/gRPC transport loop:

- `Transport channel disconnected`: `0`;
- `Seer: Component stopped`: `1`;
- `Seer shut down successfully`: `1`;
- `SIGTERM`: `0`.

Smoke r4 nadal nie spelnia pelnej bramki pozytywnej, poniewaz proces `ghost-launcher` nie udowodnil koncowego clean shutdown po SIGINT. Po zamknieciu Seer, OracleRuntime i PostBuyRuntime launcher zatrzymal sie na `Waiting for Watchdog to shut down...`, a log nadal emitowal `ReconciliationRuntime health`. Brakuje koncowych linii typu `Watchdog shut down successfully`, `All components shut down` albo rownowaznego potwierdzenia zamkniecia procesu przez runtime.

To nie jest PR17 fidelity validation burnin i nie jest research-grade evidence.

## 2. Zakres operacyjny

Zakres wykonany:

- PR #17 byl juz merged przed lokalnym wykonaniem smoke;
- PR #17 head potwierdzony:
  `eaf9cc91de83652550d36400b372aac2163f775a`;
- lokalny `main` zostal zaktualizowany do:
  `5359a6c2e1622823fc09d7b2f1506fff3360d21d`;
- przygotowano swiezy scope:
  `reports/selector/shadow-v2-fidelity-validation`;
- poprzedni scope smoke zostal przeniesiony bez kasowania do trwalego backupu:
  `/root/Gho_shadow_v2_backups/shadow-v2-fidelity-validation-before-r4-20260701T040821Z`;
- wygenerowano swiezy `pre_run_manifest.json` dla:
  `shadow-burnin-v2-fidelity-validation-logging-only-smoke-r4`;
- uruchomiono wylacznie logging-only smoke z `shadow_v2_burnin.enabled=true` i `shadow_v2_burnin.logging_only=true`;
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

## 3. Lokalny smoke config

Do smoke r4 uzyto lokalnych, niestage'owanych plikow `*.local.toml`, zeby nie zmieniac repozytoryjnych rolloutow:

- `configs/rollout/shadow-v2-validation-smoke-r4.local.toml`;
- `configs/rollout/ghost_brain_shadow_v2_validation_smoke_r4.local.toml`.

Lokalny launcher config zachowal:

- `execution_mode = "shadow"`;
- `entry_mode = "shadow_only"`;
- Seer source mode `grpc`;
- osobne sciezki runtime/log dla smoke r4.

Lokalny Ghost Brain config byl kopia pelnego `ghost-brain/ghost_brain_config.toml` z dopietym blokiem `[shadow_v2_burnin]` z profilu logging-only. Minimalny profil `configs/rollout/ghost_brain_shadow_v2_validation_logging_only.toml` sam nie zawiera `[gatekeeper_v2]`, wiec nie moze byc bezposrednim pelnym brain configiem launchera.

Sekrety i endpointy zostaly zaladowane lokalnie z istniejacego env snapshotu oraz nadpisanym aktualnym tokenem NLN w srodowisku procesu. Sekrety nie sa zapisane w tym raporcie.

## 4. Pre-run i preflight

Pre-run manifest generation:

- status: `PASS`;
- blockers: `[]`;
- run_id: `shadow-burnin-v2-fidelity-validation-logging-only-smoke-r4`.

Pre-run strict audit:

- status: `PASS`;
- blockers: `[]`.

Launcher preflight:

- status: `PASS`;
- execution mode: `Shadow`;
- entry mode: `shadow_only`;
- Gatekeeper contract: `PASS`;
- NLN gRPC app probe: `PASS`;
- trigger RPC getVersion: `PASS`;
- trigger balance: `PASS`;
- metrics port: `PASS`.

Nie stwierdzono problemu z NLN auth ani endpointem:

- `seer.grpc_endpoint` app probe zwrocil wersje `richat`;
- runtime stream: `Stream established`;
- `PermissionDenied`: `0`;
- `Account disabled`: `0`.

## 5. Runtime evidence

Runtime smoke potwierdzil:

- `All components started`: `1`;
- `Stream established`: `1`;
- `PostBuyRuntime: Shadow V2 validation smoke marker emitted`: `1`;
- `Oracle Runtime shut down successfully`: `1`;
- `PostBuyRuntime received shutdown signal`: `1`;
- `PostBuyRuntime: Shadow V2 post-run manifest generated and strict-verified`: `1`;
- `Seer: Shutdown signal received`: `1`;
- `Seer: Component stopped`: `1`;
- `Seer shut down successfully`: `1`.

Rzeczywiste artefakty Shadow V2:

| Artifact | Rows | Status |
|---|---:|---|
| `shadow_position_event_v2.jsonl` | 1 | PASS |
| `shadow_replay_v2.jsonl` | 1 | PASS |
| `shadow_lifecycle_v2.jsonl` | 1 | PASS |
| `shadow_path_density_v2.jsonl` | 7 | PASS |
| `post_run_manifest.json` | n/a | PASS |
| `shadow_v2_manifest_report.csv` | n/a | PRESENT |

## 6. Post-run manifest

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

## 7. Clean shutdown blocker

Clean shutdown nie zostal udowodniony.

Chronologia:

- `04:12:12.323Z`: `Shutdown signal received, stopping all components...`;
- `04:12:12.324Z`: `Seer: Shutdown signal received`;
- `04:12:12.324Z`: `[primary_global:primary:0] Shutdown`;
- `04:12:12.324Z`: `Ghost/Pump transport ... all workers exited`;
- `04:12:12.327Z`: `Oracle Runtime shut down successfully`;
- `04:12:12.358Z`: `Seer: Component stopped`;
- `04:12:22.500Z`: `PostBuyRuntime: Shadow V2 post-run manifest generated and strict-verified`;
- `04:12:22.501Z`: `PostBuyRuntime shut down successfully`;
- `04:12:22.501Z`: `Seer shut down successfully`;
- `04:12:22.501Z`: `Waiting for Watchdog to shut down...`;
- `04:12:41.689Z`: `ReconciliationRuntime health...`;
- `04:13:11.688Z`: `WATCHDOG | grpc_state=DISCONNECTED reconnects=0 ...`;
- `04:13:11.690Z`: `ReconciliationRuntime health...`.

Brak w logu:

- `Watchdog shut down successfully`;
- finalnego `All components shut down`;
- finalnego `Goodbye` albo rownowaznej linii konca runtime.

Zachowanie procesu:

- wyslano `SIGINT`;
- `SIGTERM`: `0`;
- `Transport channel disconnected`: `0`;
- `clean_shutdown_proven=false`;
- `forced_stop_used=NONE`;
- przy pozniejszej kontroli PID juz nie istnial, ale log nie zawiera finalnego potwierdzenia clean shutdown. Dlatego nie wolno uznac bramki za PASS.

Wniosek:

PR16C domknal Seer/gRPC shutdown loop, ale pelny launcher shutdown nadal nie ma dowodu zakonczenia. Obecny blocker przesunal sie na `Watchdog` / `ReconciliationRuntime` shutdown join albo finalizacje taskow po `Waiting for Watchdog to shut down...`.

## 8. Klasyfikacja problemu NLN/gRPC

To nie wyglada na bledny adres NLN, autoryzacje ani transport reconnect flood:

- preflight NLN app probe przeszedl;
- runtime zestawil stream;
- `Transport channel disconnected=0`;
- `reconnects=0` w watchdog line po shutdownie;
- Seer core event loop zatrzymal sie po shutdown request.

Pozostaly problem dotyczy finalizacji procesu po zamknieciu Seer/PostBuyRuntime, nie samego Seer/gRPC reconnect loop.

## 9. Guard rails

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
- nie powinno byc stage'owane.

Lokalne smoke configi:

- istnieja lokalnie jako `*.local.toml`;
- nie sa przeznaczone do commita;
- nie powinny byc stage'owane.

## 10. Nastepny wymagany krok

Przed PR17 potrzebny jest maly PR16E / PR15-fix, ale nie dla Seer/gRPC transport loop. Nowy pojedynczy blocker:

`LAUNCHER_WATCHDOG_RECONCILIATION_SHUTDOWN_WAIT`

Minimalny zakres przyszlej poprawki:

1. Podlaczyc globalny shutdown do `Watchdog` i/lub `ReconciliationRuntime`, jesli nadal zyja po zamknieciu pozostalych komponentow.
2. Upewnic sie, ze `Waiting for Watchdog to shut down...` konczy sie bounded i loguje sukces albo jawny typed failure.
3. Nie zmieniac BUY/REJECT, Gatekeeper policy, selector runtime, TX/Jito/live path, `shadow_close_only`, active close ani R51.
4. Powtorzyc logging-only smoke.
5. Wymagac:
   - `shadow_position_event_v2 rows > 0`;
   - `shadow_replay_v2 rows > 0`;
   - `shadow_lifecycle_v2 rows > 0`;
   - `shadow_path_density_v2 rows > 0`;
   - `post_run_manifest.status=PASS`;
   - post-run strict audit PASS;
   - `clean_shutdown_proven=true`;
   - brak SIGTERM;
   - brak reconnect/disconnect flood po shutdownie.

## 11. Decyzja

Smoke r4 jest wartosciowym negatywnym dowodem:

- writer/materializer/manifest path dziala;
- PR16C naprawil Seer/gRPC reconnect/disconnect flood;
- NLN gRPC i RPC sa sprawne w tym smoke;
- pelny clean shutdown procesu nadal nie jest udowodniony, ale blocker przesunal sie poza Seer/gRPC transport loop.

Final:

`PR17_REMAINS_BLOCKED`
