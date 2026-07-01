# ADR-8D: Shadow V2 Validation Harness Smoke PR16F R5 Spectrum

## Status

Accepted as core harness positive smoke evidence with Program Streams partial coverage blocker.

## D1. Problem

Po merge PR16E oraz PR20 operator defaults nalezalo powtorzyc logging-only smoke i sprawdzic, czy Shadow V2 harness realnie przechodzi pelna sciezke:

`preflight -> canonical JSONL -> derived replay/lifecycle -> density rows -> post_run_manifest PASS -> clean shutdown`

Jednoczesnie operator wymagal konfiguracji jak w starym shadow burnin:

- NLN gRPC jako glowny ingest;
- NLN Program Streams `events.nln.clr3.org:443` dla:
  - `solana.pump_fun.buy`;
  - `solana.pump_fun.buy_exact_sol_in`;
- Spectrum RPC jako endpoint RPC dla shadow burnin zamiast NLN RPC.

## D2. Decyzja

Smoke r5-spectrum klasyfikujemy jako:

`CORE_HARNESS_SMOKE_PASS_PROGRAM_STREAMS_PARTIAL_FAILURE`

Core Shadow V2 harness smoke jest zaakceptowany jako pozytywny dowod writer/materializer/manifest/shutdown.

PR17 fidelity validation burnin pozostaje zablokowany, poniewaz pelna Program Streams coverage nie zostala potwierdzona dla `solana.pump_fun.buy`.

Nie przyznajemy:

- `runtime_approval`;
- `shadow_close_only_approval`;
- `active_close_approval`;
- `research_grade`;
- `live_equivalence`;
- zgody na PR17 fidelity validation burnin.

## D3. Kontekst

R4 failowal przez residual launcher shutdown blocker po `Waiting for Watchdog to shut down...`.

PR16E mial naprawic Watchdog/Reconciliation/final launcher join.

R5-spectrum dodatkowo sprawdzal operacyjna konfiguracje z:

- NLN gRPC;
- NLN Program Streams;
- Spectrum RPC.

## D4. Dowody

Baseline:

- lokalny `main` po merge PR20 operator defaults:
  `7bb558fbad66e0974b363bd564b46f922b7becb9`.

Pre-run:

- pre-run manifest generation: `PASS`;
- pre-run strict audit: `PASS`;
- launcher preflight: `PASS`;
- Spectrum RPC `getVersion`: `PASS`, `4.1.0`;
- NLN gRPC app probe: `PASS`.

Shadow V2 evidence:

- `shadow_position_event_v2.jsonl`: `1` row;
- `shadow_replay_v2.jsonl`: `1` row;
- `shadow_lifecycle_v2.jsonl`: `1` row;
- `shadow_path_density_v2.jsonl`: `7` rows.

Manifest:

- `post_run_manifest.status=PASS`;
- blockers: `[]`;
- post-run strict audit: `PASS`.

Static guards:

- `python3 scripts/shadow_v2_validation_burnin_plan_audit.py --strict`: `PASS`;
- `python3 scripts/shadow_v2_legacy_downgrade_audit.py --strict`: `PASS`.

Shutdown evidence:

- process exited after SIGINT: okolo `10.2s`;
- `SIGTERM`: `0`;
- `Transport channel disconnected`: `0`;
- `Oracle Runtime shut down successfully`: `1`;
- `PostBuyRuntime shut down successfully`: `1`;
- `Seer shut down successfully`: `1`;
- `Watchdog shut down successfully`: `1`;
- `All components shut down successfully`: `1`;
- `Ghost Launcher shutdown complete`: `1`.

Program Streams evidence:

- endpoint: `events.nln.clr3.org:443`;
- requested topics: `2`;
- started topics: `2`;
- `ListTopics`: `PASS`;
- `topic_count`: `1074`;
- `missing_selected_topics=[]`;
- `solana.pump_fun.buy_exact_sol_in`: first message received;
- `solana.pump_fun.buy`: `NLN Subscribe request failed`, topic lane exited.

## D5. Root Cause Classification

Nie potwierdzono juz starego globalnego problemu z endpointem `events.nln.clr3.org:443`, poniewaz:

- `ListTopics` dziala;
- oba wymagane topic-i sa widoczne;
- jeden z topicow (`buy_exact_sol_in`) odbiera message.

Pozostaje osobny blocker:

`PROGRAM_STREAM_TOPIC_BUY_SUBSCRIBE_FAILED`

Na podstawie obecnego smoke nie rozstrzygamy, czy jest to blad po stronie providera, sposobu subscribe requestu w kliencie, czy specyficznego kontraktu topicu. Dla celow PR17 wystarczy fakt, ze jeden z dwoch wymaganych topicow nie dostarczyl danych.

## D6. Konsekwencje

Mozna uznac, ze PR16E domknal harness clean shutdown.

Mozna uznac, ze Shadow V2 logging-only harness potrafi wygenerowac:

- canonical event JSONL;
- derived replay JSONL;
- derived lifecycle JSONL;
- density rows;
- `post_run_manifest.status=PASS`.

Nie mozna uznac, ze pelny validation burnin jest gotowy do startu, dopoki Program Streams coverage dla `solana.pump_fun.buy` nie zostanie potwierdzona albo jawnie zdegradowana w planie coverage.

## D7. Runtime Boundary

Smoke uzywal logging-only Shadow V2 validation mode.

Nie zmieniono i nie zatwierdzono:

- BUY/REJECT;
- Gatekeeper policy;
- selector runtime;
- TX/Jito/live path;
- `shadow_close_only`;
- active close;
- RCE;
- strategy research.

## D8. Required Follow-Up

Przed PR17:

1. Zweryfikowac `solana.pump_fun.buy` Program Stream po stronie NLN albo klienta.
2. Potwierdzic pierwszy message dla obu topicow w krotkim Program Streams smoke.
3. Dopiero potem uruchomic PR17 fidelity validation burnin.

Do tego czasu:

- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `strategy_research_unblocked=false`;
- `research_grade=NOT_GRANTED`;
- `live_equivalence=NOT_GRANTED`;
- `PR17=BLOCKED_BY_PROGRAM_STREAMS_PARTIAL_COVERAGE`.
