# ADR-8D: Shadow V2 PR16B Smoke Fixes

## Status

Accepted for PR16B implementation review.

## D1. Problem

Negatywny smoke r2 po PR16A wykazał dwa blokery:

1. `shadow_position_event_v2.jsonl` generował rekord bez top-level `schema=shadow_position_event_v2`, przez co post-run manifest audit blokował artifact mimo obecności canonical row.
2. Logging-only smoke nie udowodnił clean shutdown, ponieważ główny proces nie kończył się po SIGINT i wymagał SIGTERM.

## D2. Decyzja

W PR16B przyjmujemy:

- canonical event artifact `shadow_position_event_v2.jsonl` musi mieć top-level `schema=shadow_position_event_v2`;
- canonical payload w rekordzie nadal deklaruje `canonical_payload_schema=shadow_position_v2`;
- OracleRuntime dostaje opcjonalny globalny shutdown receiver i kończy główną pętlę po sygnale shutdown;
- stary wrapper OracleRuntime i starsze testy zachowują dotychczasową semantykę przez `None`.

## D3. Kontekst

PR16A dodał deterministic smoke marker i potwierdził, że bez BUY/handoff powstają:

- canonical row;
- derived replay row;
- derived lifecycle row;
- density rows.

Smoke r2 nadal był `FAIL_BLOCKED_SCHEMA_CONTRACT_AND_SHUTDOWN`, ponieważ schema coverage i clean shutdown nie spełniły bramek.

## D4. Dowody

Testy lokalne:

- `cargo test -p ghost-brain shadow_v2_terminal_jsonl_writer_emits_canonical_event_stream -- --nocapture`
  - wynik: `PASS`;
  - potwierdza top-level `schema=shadow_position_event_v2`;
  - potwierdza payload schema `shadow_position_v2`.

- `cargo test -p ghost-launcher oracle_runtime_stops_on_shutdown_receiver -- --nocapture`
  - wynik: `PASS`;
  - potwierdza, że OracleRuntime kończy task po globalnym shutdown receiverze.

- `PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_shadow_v2_manifest_audit.py`
  - wynik: `PASS`;
  - potwierdza manifest fixture dla `shadow_position_event_v2` oraz brak self-blocked post-run manifestu.

## D5. Odrzucone alternatywy

Odrzucono zmianę manifest audit, która akceptowałaby `envelope.schema=shadow_position_v2` jako schema artifactu `shadow_position_event_v2.jsonl`.

Powód:

- mieszałaby schema artifactu event stream z schema payloadu pozycji;
- utrudniałaby przyszłe event-level validation;
- utrzymywałaby niejasny kontrakt ujawniony w smoke r2.

Odrzucono próbę maskowania shutdown flood przez log filtering.

Powód:

- problemem bramki nie był sam log flood, tylko brak zakończenia OracleRuntime przez globalny shutdown;
- clean shutdown musi być zachowaniem runtime, nie tylko zmianą logów.

## D6. Konsekwencje

Po PR16B:

- manifest audit powinien widzieć `shadow_position_event_v2` jako schema artifactu canonical JSONL;
- `canonical_payload_schema=shadow_position_v2` nadal pozwala rozpoznać typ payloadu;
- OracleRuntime nie powinien blokować clean shutdown po globalnym sygnale zatrzymania;
- PR17 nadal nie jest odblokowany bez ponowionego smoke PASS.

## D7. Inwarianty

Zachowane inwarianty:

- brak zmian BUY/REJECT;
- brak zmian Gatekeeper policy;
- brak zmian selector runtime;
- brak zmian TX/Jito/live path;
- brak enablement `shadow_close_only`;
- brak enablement active close;
- brak konsumpcji Shadow V2 przez decyzje;
- Shadow V2 pozostaje logging-only evidence path.

## D8. Bramka akceptacyjna

PR16B może być zaakceptowany kodowo po:

- przejściu testu canonical JSONL schema;
- przejściu testu OracleRuntime shutdown receiver;
- przejściu manifest audit fixtures;
- potwierdzeniu, że raw JSONL/log/R51 artifacts nie są staged.

Po merge PR16B należy powtórzyć logging-only smoke i wymagać:

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
- `live_equivalence=NOT_GRANTED`.
