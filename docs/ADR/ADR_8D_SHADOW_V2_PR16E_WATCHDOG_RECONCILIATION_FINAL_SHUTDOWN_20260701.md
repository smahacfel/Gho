# ADR-8D: Shadow V2 PR16E Watchdog/Reconciliation Final Shutdown

## Status

Accepted for PR16E implementation review.

## D1. Problem

Smoke r4 po PR16C potwierdził, że Shadow V2 logging-only harness generuje wymagane evidence rows i że Seer/gRPC transport loop nie flooduje już po shutdownie. Nadal nie można było jednak uznać clean shutdown procesu `ghost-launcher` za udowodniony.

Zaobserwowany blocker:

`FAIL_BLOCKED_LAUNCHER_WATCHDOG_RECONCILIATION_SHUTDOWN_WAIT`

Symptomy:

- po `Waiting for Watchdog to shut down...` brakowało finalnego sukcesu dla Watchdog;
- po globalnym shutdown nadal pojawiały się logi `WATCHDOG | grpc_state=DISCONNECTED reconnects=0`;
- nadal pojawiał się okresowy log `ReconciliationRuntime health`;
- brakowało końcowej linii `All components shut down successfully`;
- bez tego nie można uznać smoke r4 za pozytywny dowód gotowości PR15 harnessu.

## D2. Decyzja

W PR16E przyjmujemy wąską zmianę wyłącznie dla kończenia komponentów pomocniczych:

- `watchdog::run_with_shutdown(...)` przyjmuje opcjonalny globalny `broadcast::Receiver<()>`;
- Watchdog w swojej pętli wybiera między tickiem health logu a shutdown signal i po shutdownie kończy pętlę;
- stary `watchdog::run(...)` zostaje zachowany jako wrapper kompatybilnościowy bez shutdown receivera;
- launcher przekazuje globalny shutdown receiver do Watchdog;
- launcher joinuje komponenty przez bounded join timeout i loguje sukces dopiero po realnym zakończeniu taska;
- przy timeout launcher loguje typed bounded failure i abortuje task jako awaryjny fallback;
- `ReconciliationRuntime` health reporter dostaje lokalny stop signal i jest joinowany po wyjściu głównej pętli OracleRuntime;
- finalne `All components shut down successfully` jest logowane tylko wtedy, gdy wszystkie joiny zakończyły się bez timeoutu lub błędu.

## D3. Kontekst

PR16B naprawił schema contract i OracleRuntime shutdown receiver.

PR16C naprawił Seer/gRPC transport loop i usunął reconnect/disconnect flood po shutdownie.

PR16D smoke r4 dowiódł, że:

- `shadow_position_event_v2.jsonl` ma rows > 0;
- `shadow_replay_v2.jsonl` ma rows > 0;
- `shadow_lifecycle_v2.jsonl` ma rows > 0;
- `shadow_path_density_v2.jsonl` ma rows > 0;
- `post_run_manifest.status=PASS`;
- post-run strict audit przechodzi;
- Seer/gRPC nie flooduje po shutdownie.

Jedyny pozostały blocker r4 dotyczył Watchdog/Reconciliation/final launcher join. PR16E nie jest PR17, nie jest validation burninem i nie jest dowodem research-grade.

## D4. Dowody

Testy lokalne:

- `cargo test -p ghost-launcher watchdog_stops_on_shutdown_receiver -- --nocapture`
  - wynik: `PASS`;
  - potwierdza, że Watchdog kończy pętlę po globalnym shutdown receiverze.

- `cargo test -p ghost-launcher oracle_runtime_stops_on_shutdown_receiver -- --nocapture`
  - wynik: `PASS`;
  - potwierdza, że zmiana dla `ReconciliationRuntime` health reportera nie regresuje wcześniejszego kontraktu OracleRuntime shutdown.

- `cargo fmt --check`
  - wynik po formatowaniu: `PASS`.

Wyniki testów zawierały istniejące warningi kompilacyjne w `ghost-core`, `seer`, `ghost-brain` i `ghost-launcher`; PR16E nie naprawia globalnych warningów poza własnym zakresem.

## D5. Odrzucone alternatywy

Odrzucono wyciszenie logów Watchdog/Reconciliation.

Powód:

- problemem nie był sam tekst logu, tylko brak udowodnionego zakończenia tasków;
- clean shutdown musi wynikać z task lifecycle, a nie z maskowania objawów.

Odrzucono natychmiastowy abort Watchdog jako normalną ścieżkę.

Powód:

- smoke ma udowodnić clean shutdown;
- abort może istnieć tylko jako bounded fallback po timeout.

Odrzucono traktowanie `post_run_manifest.status=PASS` jako wystarczające.

Powód:

- manifest PASS dowodzi ścieżki evidence;
- PR16 readiness wymaga także audytowalnego zakończenia procesu bez SIGTERM i bez wiszących tasków.

## D6. Konsekwencje

Po PR16E:

- Watchdog powinien kończyć się po globalnym shutdown signal;
- ReconciliationRuntime health reporter nie powinien emitować logów po zakończeniu OracleRuntime;
- launcher ma bounded join dla komponentów i jednoznaczny finalny status;
- kolejne logging-only smoke r5 może sprawdzić pełną bramkę clean shutdown.

PR16E sam nie przyznaje:

- `runtime_approval`;
- `shadow_close_only_approval`;
- `active_close_approval`;
- `research_grade`;
- `live_equivalence`;
- zgody na PR17 fidelity validation burnin.

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

PR16E może być zaakceptowany kodowo po:

- przejściu component-level testu Watchdog shutdown;
- przejściu istniejącego OracleRuntime shutdown testu;
- przejściu `cargo fmt --check`;
- przejściu `git diff --check`;
- potwierdzeniu, że raw JSONL/log/R51 artifacts nie są staged.

Po merge PR16E należy dopiero powtórzyć logging-only smoke r5 i wymagać:

- `shadow_position_event_v2 rows > 0`;
- `shadow_replay_v2 rows > 0`;
- `shadow_lifecycle_v2 rows > 0`;
- `shadow_path_density_v2 rows > 0`;
- `post_run_manifest.status=PASS`;
- `post_run_strict_audit=PASS`;
- `clean_shutdown_proven=true`;
- brak SIGTERM;
- brak reconnect/disconnect flood po shutdownie;
- końcowy launcher join status jednoznacznie zamknięty.

Do czasu pozytywnego smoke:

- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `strategy_research_unblocked=false`;
- `research_grade=NOT_GRANTED`;
- `live_equivalence=NOT_GRANTED`;
- `PR17=BLOCKED`.
