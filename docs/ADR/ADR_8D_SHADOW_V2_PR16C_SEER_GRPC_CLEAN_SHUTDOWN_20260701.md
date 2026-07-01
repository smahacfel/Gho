# ADR-8D: Shadow V2 PR16C Seer gRPC Clean Shutdown

## Status

Accepted for PR16C implementation review.

## D1. Problem

Negatywny smoke po PR16B nadal blokował przejście do PR17, bo clean shutdown procesu `ghost-launcher` nie był jeszcze udowodniony dla pętli transportu Seer/gRPC.

Zaobserwowany symptom:

- po SIGINT globalny shutdown nie kończył pętli Seer/gRPC w sposób pewny;
- proces mógł dalej logować rozłączenia transportu, w tym `Transport channel disconnected`;
- clean exit wymagał SIGTERM;
- bez clean shutdown nie można uznać ścieżki `preflight -> writer/materializer -> post_run_manifest` za operacyjnie zamkniętą.

## D2. Decyzja

W PR16C przyjmujemy wąską zmianę wyłącznie dla shutdownu Seer/gRPC:

- `YellowstoneConnector` i `GrpcConnection` otrzymują wspólny shutdown state oraz `CancellationToken`;
- pętle reconnect, cooldown/circuit-breaker, subscribe, read loop i idle drain dostają priorytetowy branch shutdown;
- po sygnale shutdown pętla nie próbuje reconnectu i nie kontynuuje log flood;
- publiczny `Seer::request_shutdown()` propaguje sygnał do primary gRPC lane oraz funding gRPC lane;
- komponent launcherowy Seer po globalnym shutdown najpierw żąda zamknięcia Seer transportu, a następnie próbuje zjoinować core task w ograniczonym czasie;
- fallback abort pozostaje tylko jako bounded safety path, nie jako normalna ścieżka clean shutdown.

## D3. Kontekst

PR16B naprawił schema contract `shadow_position_event_v2` oraz OracleRuntime shutdown receiver. Nadal brakowało analogicznego zamknięcia po stronie Seer/gRPC.

Ten PR nie jest smoke runem, fidelity validation burninem ani PR17. Jego jedyny cel to usunięcie blokera clean shutdown przed kolejną próbą logging-only smoke r4.

## D4. Dowody

Testy lokalne:

- `cargo test -p seer grpc_connection_request_shutdown_wakes_idle_event_stream`
  - wynik: `PASS`;
  - potwierdza, że `GrpcConnection::request_shutdown()` budzi idle event stream drain i pozwala mu zakończyć się bez oczekiwania na kolejny event.

- `cargo test -p seer provider_circuit_breaker_wait_exits_on_shutdown_token`
  - wynik: `PASS`;
  - potwierdza, że circuit-breaker/cooldown wait wychodzi po shutdown token i nie wymusza kolejnej próby reconnect.

- `cargo test -p seer seer_request_shutdown_marks_primary_and_funding_grpc_lanes`
  - wynik: `PASS`;
  - potwierdza, że `Seer::request_shutdown()` ustawia shutdown dla primary gRPC lane i funding gRPC lane.

- `cargo test -p ghost-launcher seer_component_returns_after_global_shutdown_signal`
  - wynik: `PASS`;
  - potwierdza, że komponent Seer wraca po globalnym shutdown signal bez wiszenia na transport loop.

- `cargo test -p ghost-launcher oracle_runtime_stops_on_shutdown_receiver`
  - wynik: `PASS`;
  - potwierdza, że wcześniejsza ścieżka OracleRuntime shutdown z PR16B nadal działa.

- `cargo fmt --check`
  - wynik: `PASS`.

Wyniki testów zawierały istniejące warningi kompilacyjne w `ghost-core`, `seer`, `ghost-brain` i `ghost-launcher`; PR16C nie naprawia globalnych warningów poza własnym zakresem.

## D5. Odrzucone alternatywy

Odrzucono filtrowanie lub wyciszenie logu `Transport channel disconnected`.

Powód:

- problemem nie był sam tekst logu, tylko brak pewnego zakończenia pętli transportu;
- clean shutdown musi wynikać z kontroli task lifecycle, a nie z maskowania symptomów.

Odrzucono natychmiastowe abortowanie Seer core task jako podstawową ścieżkę shutdown.

Powód:

- smoke ma udowodnić clean shutdown, nie tylko wymuszone ubijanie taska;
- abort pozostaje dopuszczalny tylko po bounded timeout jako awaryjna ochrona procesu.

Odrzucono zmianę Gatekeeper, selector, TX/Jito albo Shadow V2 evidence modelu.

Powód:

- PR16C dotyczy tylko transport loop shutdown;
- zmiana decyzji, execution albo shadow evidence kontraktu byłaby scope creep.

## D6. Konsekwencje

Po PR16C:

- Seer/gRPC powinien reagować na globalny shutdown bez reconnect flood;
- launcher ma ścieżkę joinowania Seer core task bez SIGTERM;
- kolejne logging-only smoke r4 może ponownie sprawdzić full harness path;
- PR17 fidelity validation burnin pozostaje zablokowany do czasu pozytywnego smoke.

## D7. Inwarianty

Zachowane inwarianty:

- brak zmian BUY/REJECT;
- brak zmian Gatekeeper policy;
- brak zmian selector runtime;
- brak zmian TX/Jito/live path;
- brak enablement `shadow_close_only`;
- brak enablement active close;
- brak zmian R51;
- brak stage raw JSONL/log/runtime artifacts;
- brak zmian strategii;
- brak uruchomienia PR17.

## D8. Bramka akceptacyjna

PR16C może być zaakceptowany kodowo po:

- przejściu component-level testów Seer/gRPC shutdown;
- przejściu launcher-level testu global shutdown dla Seer component;
- przejściu istniejącego OracleRuntime shutdown testu;
- przejściu `cargo fmt --check`;
- przejściu `git diff --check`;
- potwierdzeniu, że raw JSONL/log/R51 artifacts nie są staged.

Po merge PR16C należy dopiero powtórzyć logging-only smoke r4 i wymagać:

- `shadow_position_event_v2 rows > 0`;
- `shadow_replay_v2 rows > 0`;
- `shadow_lifecycle_v2 rows > 0`;
- `shadow_path_density_v2 rows > 0`;
- `post_run_manifest.status=PASS`;
- `post_run_strict_audit=PASS`;
- `clean_shutdown_proven=true`.

Do czasu pozytywnego smoke:

- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `strategy_research_unblocked=false`;
- `research_grade=NOT_GRANTED`;
- `live_equivalence=NOT_GRANTED`;
- `PR17=BLOCKED`.
