# RAPORT SHADOW V2 VALIDATION HARNESS SMOKE PR16F R5 SPECTRUM 20260701

## 1. Werdykt wykonawczy

Werdykt smoke r5-spectrum po merge PR16E oraz PR20 operator defaults:

`CORE_HARNESS_SMOKE_PASS_PROGRAM_STREAMS_PARTIAL_FAILURE`

Rdzen Shadow V2 logging-only harness przeszedl:

- `shadow_position_event_v2.jsonl` rows: `1`;
- `shadow_replay_v2.jsonl` rows: `1`;
- `shadow_lifecycle_v2.jsonl` rows: `1`;
- `shadow_path_density_v2.jsonl` rows: `7`;
- `post_run_manifest.status`: `PASS`;
- post-run strict audit: `PASS`;
- clean shutdown: `PASS`;
- `SIGTERM`: `0`;
- `Transport channel disconnected`: `0`;
- final runtime line: `Ghost Launcher shutdown complete`.

Pelna konfiguracja ingestu nie jest jeszcze gotowa do PR17 fidelity validation burnin, poniewaz jeden z dwoch wymaganych NLN Program Streams topic-ow nie dostarczyl danych:

- `solana.pump_fun.buy_exact_sol_in`: `PASS`, pierwszy message odebrany;
- `solana.pump_fun.buy`: `FAIL`, lane zakonczyl sie po `NLN Subscribe request failed`.

To nie jest PR17 fidelity validation burnin, nie jest strategy proof i nie jest research-grade evidence.

## 2. Zakres operacyjny

Zakres wykonany:

- lokalny `main` po merge PR20 operator defaults:
  `7bb558fbad66e0974b363bd564b46f922b7becb9`;
- przygotowano swiezy scope:
  `reports/selector/shadow-v2-fidelity-validation-r5-spectrum`;
- wygenerowano swiezy `pre_run_manifest.json` dla:
  `shadow-burnin-v2-fidelity-validation-logging-only-smoke-r5-spectrum`;
- uruchomiono wylacznie logging-only smoke z:
  - `shadow_v2_burnin.enabled=true`;
  - `shadow_v2_burnin.logging_only=true`;
- NLN zostal uzyty dla:
  - glownego gRPC ingestu: `grpc.nln.clr3.org:443`;
  - Program Streams: `events.nln.clr3.org:443`;
- Spectrum zostal uzyty jako RPC endpoint dla shadow burnin smoke:
  - `GHOST_SEER_RPC_ENDPOINT`;
  - `GHOST_TRIGGER_RPC_URL`;
  - `GHOST_TRIGGER_SHADOW_RPC_URL`.

Zakres wykluczony:

- brak PR17 fidelity validation burnin;
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

Do smoke r5-spectrum uzyto lokalnych, niestage'owanych plikow `*.local.toml`:

- `configs/rollout/shadow-v2-validation-smoke-r5-spectrum.local.toml`;
- `configs/rollout/ghost_brain_shadow_v2_validation_smoke_r5_spectrum.local.toml`.

Lokalny launcher config zachowal:

- `execution_mode = "shadow"`;
- `entry_mode = "shadow_only"`;
- Seer source mode `grpc`;
- NLN Program Streams z dwoma topicami:
  - `solana.pump_fun.buy`;
  - `solana.pump_fun.buy_exact_sol_in`;
- osobny smoke scope i osobne porty dla r5-spectrum.

Sekrety zostaly zaladowane wylacznie z lokalnego srodowiska procesu. Sekrety nie sa zapisane w tym raporcie.

## 4. Pre-run i preflight

Pre-run manifest generation:

- status: `PASS`;
- blockers: `[]`;
- run_id: `shadow-burnin-v2-fidelity-validation-logging-only-smoke-r5-spectrum`.

Pre-run strict audit:

- status: `PASS`;
- blockers: `[]`.

Build/preflight:

- `cargo build -p ghost-launcher --bin ghost-launcher --release`: `PASS`;
- launcher preflight: `PASS`;
- execution mode: `Shadow`;
- entry mode: `shadow_only`;
- Gatekeeper contract: `PASS`;
- NLN gRPC app probe: `PASS`;
- Spectrum RPC `getVersion`: `PASS`, `4.1.0`;
- trigger balance: `PASS`;
- metrics port: `PASS`.

## 5. Runtime evidence

Runtime smoke potwierdzil:

- `All components started successfully`: `1`;
- primary gRPC stream established: `PASS`;
- `PostBuyRuntime: Shadow V2 validation smoke marker emitted`: `1`;
- `Oracle Runtime shut down successfully`: `1`;
- `PostBuyRuntime shut down successfully`: `1`;
- `Seer shut down successfully`: `1`;
- `Watchdog shut down successfully`: `1`;
- `All components shut down successfully`: `1`;
- `Ghost Launcher shutdown complete`: `1`.

Rzeczywiste artefakty Shadow V2:

| Artifact | Rows | Status |
|---|---:|---|
| `shadow_position_event_v2.jsonl` | 1 | PASS |
| `shadow_replay_v2.jsonl` | 1 | PASS |
| `shadow_lifecycle_v2.jsonl` | 1 | PASS |
| `shadow_path_density_v2.jsonl` | 7 | PASS |
| `post_run_manifest.json` | n/a | PASS |

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

Static guards:

- `python3 scripts/shadow_v2_validation_burnin_plan_audit.py --strict`: `PASS`;
- `python3 scripts/shadow_v2_legacy_downgrade_audit.py --strict`: `PASS`.

## 7. Clean shutdown

Clean shutdown zostal udowodniony.

Chronologia:

- wyslano jeden `SIGINT`;
- proces `ghost-launcher` zakonczyl sie po okolo `10.2s` od sygnalu shutdown;
- `SIGTERM`: `0`;
- `Transport channel disconnected`: `0`;
- `Watchdog shut down successfully`: `1`;
- `All components shut down successfully`: `1`;
- `Ghost Launcher shutdown complete`: `1`.

Wniosek:

PR16E domknal residual shutdown blocker z r4. Dla r5-spectrum nie ma juz blockera Watchdog/Reconciliation/final launcher join.

## 8. NLN gRPC i Program Streams

NLN gRPC ingest dzialal:

- `seer.grpc_endpoint`: `grpc.nln.clr3.org:443`;
- runtime stream established: `PASS`;
- `PermissionDenied`: `0`;
- `Account disabled`: `0`;
- `Transport channel disconnected`: `0`.

NLN Program Streams endpoint `events.nln.clr3.org:443` nie wykazuje juz starego problemu typu 502/ListTopics/provisioning:

- Program Streams started topic count: `2`;
- `ListTopics`: `PASS`;
- `topic_count`: `1074`;
- `missing_selected_topics=[]`.

Jednoczesnie pelna dwutopic coverage nie jest potwierdzona:

| Topic | Evidence | Status |
|---|---|---|
| `solana.pump_fun.buy_exact_sol_in` | first message received | PASS |
| `solana.pump_fun.buy` | `NLN Subscribe request failed`, lane exited | FAIL |

Nie mozna na podstawie tego smoke przesadzic, czy przyczyna `solana.pump_fun.buy` lezy po stronie providera, sposobu subscribe requestu, czy szczegolowej semantyki topicu. Mozna natomiast powiedziec, ze nie jest to juz ten sam globalny problem z endpointem `events.nln.clr3.org:443`, bo `ListTopics` dziala, topic-i sa widoczne, a drugi topic odbiera dane.

## 9. Znaczenie dla PR17

Rdzen Shadow V2 logging-only harness spelnil bramki smoke:

- canonical rows > 0;
- replay rows > 0;
- lifecycle rows > 0;
- density rows > 0;
- `post_run_manifest.status=PASS`;
- post-run strict audit PASS;
- clean shutdown proven;
- no SIGTERM;
- no reconnect/disconnect flood.

Mimo tego PR17 fidelity validation burnin pozostaje zablokowany do czasu rozstrzygniecia Program Streams coverage dla `solana.pump_fun.buy`.

Powod:

pelna konfiguracja burnin dla maksymalnej coverage BCV wymaga obu topicow:

- `solana.pump_fun.buy`;
- `solana.pump_fun.buy_exact_sol_in`.

Smoke r5-spectrum udowodnil tylko jeden z nich jako live-receiving.

## 10. Guard rails

Status approval pozostaje:

- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `strategy_research_unblocked=false`;
- `research_grade=NOT_GRANTED`;
- `live_equivalence=NOT_GRANTED`;
- `PR17 fidelity validation burnin=BLOCKED_BY_PROGRAM_STREAMS_PARTIAL_COVERAGE`.

Raw evidence:

- istnieje lokalnie w `reports/selector/shadow-v2-fidelity-validation-r5-spectrum`;
- nie jest przeznaczone do commita;
- nie powinno byc stage'owane.

Lokalne smoke configi:

- istnieja lokalnie jako `*.local.toml`;
- nie sa przeznaczone do commita jako runtime rollout;
- sluzyly wylacznie do tej operacyjnej proby smoke.

## 11. Wymagane follow-up

Przed PR17 nalezy wykonac jeden z ponizszych krokow:

1. powtorzyc krotki Program Streams smoke po stronie `events.nln.clr3.org:443` i potwierdzic pierwszy message dla obu topicow; albo
2. eskalowac do NLN konkretny przypadek:
   - endpoint: `events.nln.clr3.org:443`;
   - topic: `solana.pump_fun.buy`;
   - symptom: topic widoczny w `ListTopics`, ale `Subscribe` konczy sie `NLN Subscribe request failed`;
   - kontrast: `solana.pump_fun.buy_exact_sol_in` odbiera pierwszy message w tej samej konfiguracji.

Do czasu tego rozstrzygniecia:

- nie uruchamiac PR17;
- nie traktowac r5-spectrum jako pelnego validation burnin;
- nie odblokowywac strategy research.
