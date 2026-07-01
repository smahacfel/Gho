# ADR-8D: Shadow V2 Validation Harness Smoke PR16F R5 Spectrum

## Status

Accepted as positive logging-only smoke evidence.

## D1. Problem

Po PR16E i PR20 trzeba bylo potwierdzic, czy Shadow V2 logging-only validation harness dziala w pelnej konfiguracji operacyjnej:

`pre_run_manifest -> runtime preflight -> canonical JSONL -> derived replay/lifecycle -> density rows -> post_run_manifest PASS -> clean shutdown`

Poprzedni r5 mial pozytywny core harness smoke, ale nie potwierdzal pelnej NLN Program Streams coverage, bo `solana.pump_fun.buy` zakonczyl lane z `NLN Subscribe request failed`. Po stronie providera potwierdzono limit streamow i podniesiono limit. Nalezalo powtorzyc smoke i rozstrzygnac, czy blocker nadal istnieje.

## D2. Decyzja

Powtorzony smoke r5-spectrum klasyfikujemy jako:

`SHADOW_V2_LOGGING_ONLY_SMOKE_PASS`

Akceptujemy ten wynik jako dowod, ze:

- Shadow V2 logging-only harness inicjuje sie poprawnie;
- canonical writer zapisuje `shadow_position_event_v2.jsonl`;
- derived replay/lifecycle materializuja wiersze;
- density writer materializuje wiersze;
- post-run manifest jest generowany i strict-verified;
- clean shutdown konczy proces bez SIGTERM;
- NLN Program Streams dostarczaja pierwszy message dla obu wymaganych topicow:
  - `solana.pump_fun.buy`;
  - `solana.pump_fun.buy_exact_sol_in`.

Nie przyznajemy:

- `runtime_approval`;
- `shadow_close_only_approval`;
- `active_close_approval`;
- `research_grade`;
- `live_equivalence`;
- strategy research unblocked.

PR17 fidelity validation burnin nie zostal uruchomiony. Pozytywny smoke usuwa blocker przygotowawczy, ale PR17 pozostaje osobnym etapem wymagajacym osobnej decyzji operatora.

## D3. Kontekst

Konfiguracja smoke:

- NLN gRPC jako glowny ingest: `grpc.nln.clr3.org:443`;
- NLN Program Streams endpoint: `events.nln.clr3.org:443`;
- Program Streams topics:
  - `solana.pump_fun.buy`;
  - `solana.pump_fun.buy_exact_sol_in`;
- Spectrum RPC jako RPC endpoint dla shadow burnin smoke;
- `shadow_v2_burnin.enabled=true`;
- `shadow_v2_burnin.logging_only=true`.

Lokalne smoke configi `*.local.toml` nie sa czescia tego PR i nie powinny byc stage'owane.

## D4. Dowody

Baseline:

- branch PR: `codex/shadow-v2-pr16f-r5-spectrum-smoke-report`;
- poprzedni head raportu: `704110409b50c846a31f74af67e3e8e8cb1b1f58`;
- lokalny main po PR20: `7bb558fbad66e0974b363bd564b46f922b7becb9`.

Pre-run:

- pre-run manifest generation: `PASS`;
- pre-run strict audit: `PASS`;
- `cargo build -p ghost-launcher --bin ghost-launcher --release`: `PASS`.

Shadow V2 evidence:

- `shadow_position_event_v2.jsonl`: `1` row;
- `shadow_replay_v2.jsonl`: `1` row;
- `shadow_lifecycle_v2.jsonl`: `1` row;
- `shadow_path_density_v2.jsonl`: `7` rows.

Post-run manifest:

- `post_run_manifest.status=PASS`;
- blockers: `[]`;
- schema coverage:
  - `shadow_position_event_v2`: `1`;
  - `shadow_replay_v2`: `1`;
  - `shadow_lifecycle_v2`: `1`;
  - `shadow_path_density_v2`: `7`;
- independent post-run strict audit: `PASS`.

Shutdown evidence:

- `SIGTERM`: `0`;
- `Transport channel disconnected`: `0`;
- `Oracle Runtime shut down successfully`: `1`;
- `PostBuyRuntime shut down successfully`: `1`;
- `Seer shut down successfully`: `1`;
- `Watchdog shut down successfully`: `1`;
- `All components shut down successfully`: `1`;
- `Ghost Launcher shutdown complete`: `1`.

Program Streams evidence w aktualnym oknie smoke od `2026-07-01T22:50:33+00:00`:

- endpoint: `events.nln.clr3.org:443`;
- requested topics: `2`;
- started topics: `2`;
- `ListTopics`: `PASS`;
- `topic_count`: `1074`;
- `missing_selected_topics=[]`;
- `solana.pump_fun.buy`: first message received;
- `solana.pump_fun.buy_exact_sol_in`: first message received;
- `NLN Subscribe request failed`: `0`;
- `status: 502`: `0`;
- `Bad Gateway`: `0`;
- `http2`: `0`.

## D5. Root Cause Classification

Poprzedni blocker:

`PROGRAM_STREAM_TOPIC_BUY_SUBSCRIBE_FAILED`

nie wystapil w aktualnym smoke po zmianie limitu po stronie providera.

Aktualna klasyfikacja:

`PROGRAM_STREAMS_FULL_COVERAGE_CONFIRMED_FOR_SMOKE`

Nie dowodzi to jeszcze research-grade fidelity. Dowodzi tylko, ze logging-only harness i dwutopic Program Streams coverage sa operacyjnie gotowe do nastepnego etapu walidacyjnego.

## D6. Konsekwencje

Mozna uznac, ze PR16F domyka smoke przygotowawczy:

- writer/materializer/manifest/shutdown: PASS;
- Program Streams two-topic first-message coverage: PASS;
- post-run strict audit: PASS.

Nie mozna uznac, ze:

- Shadow V2 jest research-grade;
- Shadow V2 jest live-equivalent;
- PR17 zostal wykonany;
- strategia zostala odblokowana;
- runtime approval jest prawdziwy.

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

Approval flags pozostaja:

- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `strategy_research_unblocked=false`.

## D8. Required Follow-Up

Nastepny etap:

1. Po merge tego report-only PR operator moze osobno zdecydowac o PR17 fidelity validation burnin.
2. PR17 ma pozostac validation burnin, nie strategy proof.
3. Raw JSONL/log/runtime artifacts z r5-spectrum nie moga zostac stage'owane.
4. Po PR17 wymagane beda rekonstrukcje, reconciliation, density i manifest audit reports.

Do czasu PR17:

- `research_grade=NOT_GRANTED`;
- `live_equivalence=NOT_GRANTED`;
- stare raporty nadal nie sa proof live PnL, executable fills, slippage ani landing outcome.
