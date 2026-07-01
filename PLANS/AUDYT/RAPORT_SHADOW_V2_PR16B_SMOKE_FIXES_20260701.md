# RAPORT SHADOW V2 PR16B SMOKE FIXES 20260701

## 1. Werdykt

Werdykt PR16B:

`PR16B_FIX_READY_FOR_REPEATED_SMOKE`

PR16B adresuje dwa blokery wykazane w negatywnym raporcie smoke r2:

- mismatch schemy w `shadow_position_event_v2.jsonl`;
- brak czystego shutdownu OracleRuntime po sygnale globalnego zatrzymania.

Ten raport nie przyznaje jeszcze pozytywnego smoke PASS, research-grade ani live-equivalence. Po merge PR16B wymagane jest ponowienie logging-only smoke.

## 2. Zakres

Zakres zmian:

- canonical event wrapper `ShadowPositionEventV2` emituje top-level `schema=shadow_position_event_v2`;
- payload pozostaje jawnie opisany jako `canonical_payload_schema=shadow_position_v2`;
- OracleRuntime otrzymuje opcjonalny `broadcast::Receiver<()>` globalnego shutdownu;
- `main.rs` przekazuje subskrypcję globalnego shutdownu do OracleRuntime;
- starsze wywołania testowe bez globalnego shutdownu dostają jawne `None`.

Poza zakresem:

- brak zmian BUY/REJECT;
- brak zmian Gatekeeper policy;
- brak zmian selector runtime;
- brak zmian TX/Jito/live path;
- brak włączenia `shadow_close_only`;
- brak włączenia active close;
- brak PR17 fidelity validation burnin;
- brak strategii, RCE proof, selector proof albo edge proof.

## 3. Naprawa schemy canonical JSONL

Problem smoke r2:

`shadow_position_event_v2.jsonl` zawierał payload z `envelope.schema=shadow_position_v2`, ale nie miał top-level `schema=shadow_position_event_v2`. Manifest audit oczekiwał schemy artifactu, więc `post_run_manifest.json` był `BLOCKED`.

Decyzja PR16B:

- artifact event stream ma własną top-level schemę `shadow_position_event_v2`;
- payload position pozostaje opisany przez `canonical_payload_schema=shadow_position_v2`;
- stara deserializacja pozostaje kompatybilna przez `serde(default)`.

To rozdziela:

- schema artifactu JSONL: `shadow_position_event_v2`;
- schema canonical payloadu: `shadow_position_v2`.

## 4. Naprawa shutdownu OracleRuntime

Problem smoke r2:

Runtime przyjmował shutdown signal, PostBuyRuntime zaczynał drain i generował post-run manifest, ale proces nie kończył się po dwóch SIGINT. Główny task czekał na OracleRuntime, a OracleRuntime nie miał bezpośredniego globalnego shutdown receivera.

Decyzja PR16B:

- `start_oracle_runtime_task_with_funding_availability(...)` przyjmuje opcjonalny `shutdown_rx`;
- gdy `shutdown_rx` dostanie signal albo zostanie zamknięty, OracleRuntime opuszcza główną pętlę event loop;
- wrapper `start_oracle_runtime_task(...)` zachowuje kompatybilne zachowanie i przekazuje `None`;
- `main.rs` subskrybuje globalny shutdown channel i przekazuje go do OracleRuntime.

Zmiana dotyczy tylko ścieżki zatrzymania. Nie zmienia decyzji, polityki, submitu ani konsumpcji Shadow V2.

## 5. Dowody lokalne

Uruchomione walidacje:

- `cargo test -p ghost-brain shadow_v2_terminal_jsonl_writer_emits_canonical_event_stream -- --nocapture`
  - status: `PASS`;
  - potwierdza top-level `schema=shadow_position_event_v2`;
  - potwierdza `canonical_payload_schema=shadow_position_v2`.

- `cargo test -p ghost-launcher oracle_runtime_stops_on_shutdown_receiver -- --nocapture`
  - status: `PASS`;
  - potwierdza, że OracleRuntime kończy task po sygnale shutdown receivera.

- `PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_shadow_v2_manifest_audit.py`
  - status: `PASS`;
  - potwierdza, że fixture manifest audit akceptuje schemę `shadow_position_event_v2` i nie tworzy self-blocked post-run manifestu.

## 6. Granice runtime

PR16B nie nadaje żadnych approval flags:

- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `strategy_research_unblocked=false`;
- `research_grade=NOT_GRANTED`;
- `live_equivalence=NOT_GRANTED`;
- `PR17 fidelity validation burnin=BLOCKED`.

## 7. Następny krok

Po merge PR16B trzeba powtórzyć logging-only smoke.

Wymagany pozytywny wynik smoke:

- `shadow_position_event_v2 rows > 0`;
- `shadow_replay_v2 rows > 0`;
- `shadow_lifecycle_v2 rows > 0`;
- `shadow_path_density_v2 rows > 0`;
- `post_run_manifest.status=PASS`;
- `post_run_strict_audit=PASS`;
- `clean_shutdown_proven=true`.

Do tego czasu PR17 pozostaje zablokowany.
