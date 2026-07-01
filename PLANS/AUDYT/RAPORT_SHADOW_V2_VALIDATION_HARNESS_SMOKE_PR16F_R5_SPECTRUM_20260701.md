# RAPORT SHADOW V2 VALIDATION HARNESS SMOKE PR16F R5 SPECTRUM 20260701

## 1. Werdykt wykonawczy

Werdykt po powtorzonym logging-only smoke r5-spectrum wykonanym po podniesieniu limitu NLN Program Streams:

`SHADOW_V2_LOGGING_ONLY_SMOKE_PASS`

Aktualny smoke potwierdzil kompletna sciezke Shadow V2 validation harness:

`pre_run_manifest -> launcher preflight -> canonical JSONL -> derived replay/lifecycle -> density rows -> post_run_manifest PASS -> clean shutdown`

Najwazniejsze bramki:

- `shadow_position_event_v2.jsonl` rows: `1`;
- `shadow_replay_v2.jsonl` rows: `1`;
- `shadow_lifecycle_v2.jsonl` rows: `1`;
- `shadow_path_density_v2.jsonl` rows: `7`;
- `post_run_manifest.status`: `PASS`;
- post-run strict audit: `PASS`;
- clean shutdown: `PASS`;
- `SIGTERM`: `0`;
- `Transport channel disconnected`: `0`;
- `NLN Subscribe request failed`: `0` w aktualnym oknie smoke;
- `solana.pump_fun.buy`: first message received;
- `solana.pump_fun.buy_exact_sol_in`: first message received;
- final runtime line: `Ghost Launcher shutdown complete`.

To nadal nie jest PR17 fidelity validation burnin, nie jest strategy proof i nie jest research-grade evidence. Jest to pozytywne domkniecie operacyjnego smoke writer/materializer/manifest/shutdown oraz Program Streams coverage dla dwoch wymaganych topicow.

## 2. Aktualne miejsce w procesie

Jestesmy po:

- PR15: logging-only Shadow V2 harness;
- PR16A: deterministic smoke marker;
- PR16B/PR16C/PR16E: schema/shutdown fixes;
- PR20: jawne operator defaults dla NLN + Spectrum;
- PR16F r5-spectrum smoke repeat po zwiekszeniu limitu Program Streams przez providera.

Aktualny wynik oznacza:

- blocker `FAIL_BLOCKED_NO_CANONICAL_V2_EVIDENCE`: zamkniety;
- blocker schema/manifest: zamkniety;
- blocker Seer/gRPC disconnect flood: zamkniety;
- blocker Watchdog/Reconciliation shutdown wait: zamkniety;
- blocker Program Streams partial coverage dla `solana.pump_fun.buy`: zamkniety w aktualnym smoke.

Nastepny etap moze byc dopiero PR17 fidelity validation burnin, ale wymaga osobnej dyspozycji. PR17 nie zostal uruchomiony w ramach tego zadania.

## 3. Zakres operacyjny smoke

Zakres wykonany:

- branch raportowy: `codex/shadow-v2-pr16f-r5-spectrum-smoke-report`;
- commit bazowy raportowego PR przed aktualizacja: `704110409b50c846a31f74af67e3e8e8cb1b1f58`;
- lokalny main po merge PR20 operator defaults:
  `7bb558fbad66e0974b363bd564b46f922b7becb9`;
- przygotowano swiezy scope:
  `reports/selector/shadow-v2-fidelity-validation-r5-spectrum`;
- poprzedni lokalny scope r5 przeniesiono do lokalnego backupu:
  `reports/selector/_local_smoke_backups/`;
- wygenerowano swiezy `pre_run_manifest.json` dla:
  `shadow-burnin-v2-fidelity-validation-logging-only-smoke-r5-spectrum`;
- uruchomiono wylacznie logging-only smoke z:
  - `shadow_v2_burnin.enabled=true`;
  - `shadow_v2_burnin.logging_only=true`;
- NLN zostal uzyty dla:
  - glownego gRPC ingestu: `grpc.nln.clr3.org:443`;
  - Program Streams: `events.nln.clr3.org:443`;
- Spectrum zostal uzyty jako RPC endpoint dla shadow burnin smoke.

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

## 4. Lokalny smoke config

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
- Program Streams `max_streams=2`;
- osobny smoke scope i osobne porty dla r5-spectrum.

Sekrety nie sa zapisane w tym raporcie.

## 5. Pre-run i preflight

Pre-run manifest generation:

- status: `PASS`;
- blockers: `[]`;
- run_id: `shadow-burnin-v2-fidelity-validation-logging-only-smoke-r5-spectrum`.

Pre-run strict audit:

- status: `PASS`;
- blockers: `[]`.

Build/preflight:

- `cargo build -p ghost-launcher --bin ghost-launcher --release`: `PASS`;
- launcher start: `PASS`;
- execution mode: `Shadow`;
- entry mode: `shadow_only`;
- NLN gRPC endpoint widoczny w runtime: `grpc.nln.clr3.org:443`;
- Spectrum RPC endpoint widoczny w runtime configu.

## 6. Runtime evidence

Aktualne okno smoke zaczelo sie o:

`2026-07-01T22:50:33+00:00`

W logu z tego okna potwierdzono:

- `All components started successfully`: `1`;
- primary gRPC ingest emitowal transakcje: `PASS`;
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

## 7. Post-run manifest

Post-run manifest zapisany przez harness:

- status: `PASS`;
- blockers: `[]`;
- artifact_count: `5`;
- schema coverage:
  - `shadow_position_event_v2`: `1`;
  - `shadow_replay_v2`: `1`;
  - `shadow_lifecycle_v2`: `1`;
  - `shadow_path_density_v2`: `7`.

Niezalezny post-run strict audit:

- command:
  `python3 scripts/shadow_v2_manifest_audit.py --scope-root reports/selector/shadow-v2-fidelity-validation-r5-spectrum --manifest-phase post_run --schema-manifest reports/selector/shadow_v2_required_schema_manifest.csv --acceptance-gates reports/selector/shadow_v2_acceptance_gates.csv --strict`;
- status: `PASS`;
- blockers: `[]`;
- schema coverage zgodne z artefaktami.

## 8. Clean shutdown

Clean shutdown zostal udowodniony.

Chronologia:

- wyslano jeden `SIGINT`;
- `SIGTERM`: `0`;
- `Transport channel disconnected`: `0`;
- `Watchdog shut down successfully`: `1`;
- `All components shut down successfully`: `1`;
- `Ghost Launcher shutdown complete`: `1`.

Wniosek:

Nie ma juz blockera Watchdog/Reconciliation/final launcher join w aktualnym smoke.

## 9. NLN gRPC i Program Streams

NLN gRPC ingest dzialal:

- `seer.grpc_endpoint`: `grpc.nln.clr3.org:443`;
- runtime stream established: `PASS`;
- emitowal `PoolTransaction` przez `grpc_global_stream`;
- `PermissionDenied`: `0`;
- `Account disabled`: `0`;
- `Transport channel disconnected`: `0`.

NLN Program Streams endpoint:

- endpoint: `events.nln.clr3.org:443`;
- requested topics: `2`;
- allowed stream count: `2`;
- started topics:
  - `solana.pump_fun.buy`;
  - `solana.pump_fun.buy_exact_sol_in`;
- `ListTopics`: `PASS`;
- `topic_count`: `1074`;
- `missing_selected_topics=[]`.

Dwutopic coverage w aktualnym smoke:

| Topic | Evidence | Status |
|---|---|---|
| `solana.pump_fun.buy` | first message received | PASS |
| `solana.pump_fun.buy_exact_sol_in` | first message received | PASS |

Negatywne sygnaly w aktualnym oknie smoke:

| Pattern | Count |
|---|---:|
| `NLN Subscribe request failed` | 0 |
| `Transport channel disconnected` | 0 |
| `status: 502` | 0 |
| `Bad Gateway` | 0 |
| `http2` | 0 |

W tym samym datowanym pliku logu nadal istnieje starszy wpis z poprzedniego smoke o `2026-07-01T18:54`, gdzie `solana.pump_fun.buy` failowal. Ten starszy wpis nie nalezy do aktualnego okna smoke. Dla aktualnego okna od `2026-07-01T22:50:33` liczba `NLN Subscribe request failed` wynosi `0`.

## 10. Znaczenie dla PR17

Pozytywny smoke r5-spectrum zamyka warunek operacyjny wymagany przed PR17:

- canonical rows > 0;
- replay rows > 0;
- lifecycle rows > 0;
- density rows > 0;
- `post_run_manifest.status=PASS`;
- post-run strict audit PASS;
- clean shutdown proven;
- no SIGTERM;
- no reconnect/disconnect flood;
- `solana.pump_fun.buy` first message received;
- `solana.pump_fun.buy_exact_sol_in` first message received.

To nie oznacza, ze PR17 zostal wykonany. Oznacza tylko, ze smoke harness i Program Streams coverage nie blokuja juz samego przygotowania PR17.

PR17 powinien pozostac osobnym, kontrolowanym etapem fidelity validation burnin. Nie wolno interpretowac tego smoke jako:

- research-grade validation;
- strategy proof;
- RCE proof;
- live-equivalent evidence;
- approval do runtime.

## 11. Guard rails

Status approval pozostaje:

- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `strategy_research_unblocked=false`;
- `research_grade=NOT_GRANTED`;
- `live_equivalence=NOT_GRANTED`.

Raw evidence:

- istnieje lokalnie w `reports/selector/shadow-v2-fidelity-validation-r5-spectrum`;
- nie jest przeznaczone do commita;
- nie powinno byc stage'owane.

Lokalne backupy:

- istnieja lokalnie w `reports/selector/_local_smoke_backups/`;
- nie sa przeznaczone do commita.

Lokalne smoke configi:

- istnieja lokalnie jako `*.local.toml`;
- nie sa przeznaczone do commita jako runtime rollout;
- sluzyly wylacznie do tej operacyjnej proby smoke.

## 12. Wymagane follow-up

Najblizszy sensowny krok po merge raportu PR16F:

1. nie uruchamiac strategii;
2. nie nadawac runtime approval;
3. przygotowac osobna decyzje operatora dla PR17 fidelity validation burnin;
4. PR17 powinien nadal byc validation burnin, nie strategy proof;
5. po PR17 dopiero uruchomic rekonstrukcje/reconciliation/density audit reports.

Do czasu PR17:

- Shadow V2 ma pozytywny logging-only harness smoke;
- Shadow V2 nie ma jeszcze research-grade verdict;
- Shadow V2 nie ma live-equivalence verdict;
- stare raporty nadal nie moga byc cytowane jako proof live PnL, executable fills, live slippage behavior ani landing outcome.
