# Hot-Path Metrics Runbook

Ten runbook opisuje minimalny zestaw metryk operacyjnych dla hot-pathu Ghost pipeline.
Procedura operatorska start/stop/restart/abort jest w [`docs/RUNBOOK_PRODUCTION_ROLLOUT.md`](/root/Gho/docs/RUNBOOK_PRODUCTION_ROLLOUT.md).

## Durability / Recovery

- `runtime_durability_mode{mode=...}`
  Aktywny profil durability rozstrzygnięty na starcie (`disabled`, `wal_only`, `snapshot_only`, `snapshot_and_wal`).
  Dla produkcyjnego burn-in i dual rollout oczekiwane jest `snapshot_and_wal`.

- `shadow_ledger_restore_duration_ms`
  Czas odtworzenia `ShadowLedger` z najnowszego snapshotu.
  Brak prób restore przy restarcie oznacza, że startup nie użył oczekiwanego snapshotu.

- `wal_replay_duration_ms`
  Czas replay WAL po załadowaniu snapshotu albo przy cold starcie.
  Rosnący trend bez zmiany ruchu oznacza, że replay ma zbyt duży ogon albo problemy IO.

- `runtime_recovery_mode{mode=...}`
  Faktyczny tryb recovery po starcie (`snapshot_plus_wal`, `wal_only`, `snapshot_only`, `cold_start`).
  Restart paper/dual nie powinien niespodziewanie lądować na `cold_start`.

## Ingestion

- `ingestion_latency_ms`
  Czas od odebrania `PumpEvent` do wyemitowania `GeyserEvent` po stronie transportu.
  Wzrost oznacza backlog w dual-lane drain lub koszt dekodowania.

- `parser_malformed_tx_rate`
  Udział malformed/raw-decode errors w całości prób dekodowania tx przez transport/parser.
  Wzrost zwykle wskazuje na uszkodzony feed, niekompatybilny wire format albo błędny payload.

- `ghost.pump.stall_rate`
  Udział stall-driven reconnectów w całkowitej liczbie reconnectów gRPC.
  Rosnąca wartość oznacza, że transport częściej zrywa z powodu ciszy streamu niż zwykłych reconnectów.

- `ghost.pump.provider_stall_total{provider=...}`
  Liczba stalli przypisanych do konkretnego providera.

- `ghost.pump.provider_state{provider=...}`
  Stan circuit-breakera providera.
  `0=closed`, `1=half_open`, `2=open`.

## FSC authoritative funding lane

- `ghost.pump.reconnects{source_label=...}`
  Lane-scoped licznik reconnectów gRPC.
  Dla FSC bake patrz osobno na `source_label=grpc_funding_lane_full_chain`, a nie tylko na zagregowany stan primary lane.

- `ghost.pump.stalls{source_label=...}` oraz `ghost.pump.silent_stalls{source_label=...}`
  Lane-scoped stall-driven reconnects.
  Wzrost na `grpc_funding_lane_full_chain` przy stabilnym `grpc_global_stream` oznacza problem authoritative funding lane, nie całego trade path.

- `ghost.pump.stall_rate{source_label=...}`
  Udział stall-driven reconnectów w reconnectach dla danego lane.
  Dla bake oczekiwane jest osobne śledzenie `grpc_global_stream` vs `grpc_funding_lane_full_chain`.

- `ghost.pump.provider_stall_total{provider=...,source_label=...}`
  Liczba stalli przypisanych do konkretnego providera i konkretnego lane.

- `ghost.pump.provider_state{provider=...,source_label=...}`
  Stan circuit-breakera providera rozdzielony per lane.
  `0=closed`, `1=half_open`, `2=open`.

- `seer_funding_transfer_observations_total{lane=...,coverage=...}`
  Licznik funding transferów wyemitowanych przez Seer po klasyfikacji lane/provenance.
  To jest podstawowy dowód, czy runtime widzi filtered observations, authoritative observations, czy oba typy.

- `fsc_authoritative_funding_stream_available`
  Launcher/runtime-side gauge dostępności authoritative funding lane.
  `0` oznacza, że `FSC` musi pozostać fail-closed niezależnie od wcześniejszych obserwacji indeksu.

- `fsc_warmup_ready`
  Gauge gotowości `FundingSourceIndex`.
  `1` dopiero wtedy, gdy authoritative lane jest zdrowy i runtime widział już co najmniej jeden authoritative funding transfer (`full_chain_coverage=true`).

- `fsc_coverage_window_ready`
  Gauge mocniejszego kontraktu coverage-horizon dla `FSC`.
  `1` dopiero wtedy, gdy authoritative lane pozostawał zdrowy nieprzerwanie przez całe `funding_lookback_window_s`; samo `fsc_warmup_ready=1` nie wystarcza do live BUY.

- `fsc_coverage_window_remaining_ms`
  Ile czasu ciągłego zdrowia authoritative lane brakuje jeszcze do otwarcia coverage-horizon gate.
  Przy `fsc_coverage_window_ready=1` gauge powinien spaść do `0`.

- `fsc_authoritative_buy_gate_open`
  Runtime-side gauge finalnego gate dla authoritative/live BUY.
  `0` oznacza, że obserwacja, scoring i shadow mogą działać dalej, ale live BUY ma pozostać fail-closed.

- `fsc_lookup_hit_rate`
  Skumulowany hit-rate lookupów `FSC` liczony jako `fsc_lookup_hits_total / (hits + misses)`.
  Interpretować dopiero razem z `fsc_warmup_ready=1`, `fsc_coverage_window_ready=1` i sensowną próbką lookupów.

- `fsc_lookup_hits_total` / `fsc_lookup_misses_total`
  Jawne liczniki lookupów `FSC`.
  Sam hit-rate bez surowych liczników nie wystarcza do oceny jakości coverage.

- `fsc_index_entries`
  Liczba recipient entries trzymanych przez bounded `FundingSourceIndex`.

- `fsc_index_per_recipient_overflows_total`
  Liczba rekordów wyrzuconych, bo jeden recipient przekroczył swój bounded cap.

- `fsc_index_global_evictions_total`
  Liczba recipient entries wyrzuconych przez TTL/global cap.

- `fsc_prune_duration_ms`
  Koszt prune passów bounded indeksu.
  Rosnący trend oznacza presję rozmiaru okna lub zbyt szeroką próbkę authoritative lane.

### FSC decision / diagnostics split

- `logs/decisions.jsonl/gatekeeper_v2_buys.jsonl -> funding_source_concentration`
  To jest wyłącznie kanoniczna, zmaterializowana wartość `FSC` widziana przez policy path.

- `logs/decisions.jsonl/gatekeeper_v2_buys.jsonl -> sybil_metric_degraded_reasons[]`
  To są powody fail-closed / cold-index / insufficient-known-sources dla `FSC`.

- Lane health i readiness nie są wywnioskowywane z samego `funding_source_concentration`.
  Do upstream diagnostyki authoritative funding lane używaj metryk powyżej (`ghost.pump.*{source_label=...}`, `seer_funding_transfer_observations_total`, `fsc_authoritative_funding_stream_available`, `fsc_warmup_ready`, `fsc_coverage_window_ready`, `fsc_authoritative_buy_gate_open`).

## Event Bus

- `eventbus_active_receivers`
  Aktualna liczba aktywnych receiverów broadcast busa.
  Nagły spadek oznacza, że któryś konsument odpadł.

- `eventbus_lag_total{consumer=...}`
  Liczba eventów utraconych przez `RecvError::Lagged`.
  Każdy wzrost to realna utrata ciągłości konsumenta.

## Shadow Ledger / Enrichment

- `shadow_ledger_age_ms`
  Globalny rozkład wieku snapshotów/curve state przy lookupach ShadowLedgera.

- `shadow_ledger_enrichment_latency_ms`
  Czas launcherowego enrichmentu `ShadowLedger -> PoolTransaction`.

- `shadow_ledger_enrichment_snapshot_age_ms`
  Wiek snapshotu użytego do enrichmentu w chwili użycia.

- `shadow_ledger_enrichment_total{fresh=...}`
  Licznik enrichmentów rozdzielony na curve quality actionable (`fresh=true`) vs non-actionable (`fresh=false`).
  Gdy enrichment rzeczywiscie wchodzi w `result=shadow_fallback`, rownolegle inkrementowany jest tez wspolny licznik `shadow_truth_fallback_total{site="tx_curve_enrichment_shadow"}`.

- `shadow_ledger_curve_freshness_total{state=unknown|stale|fresh|committed}`
  Jawny rozkład klasyfikacji quality curve rozwiązywanej przez hot path.

- `shadow_ledger_curve_finality_total{state=speculative|provisional|finalized}`
  Rozkład finality curve state widzianej przez launcher przed Gatekeeperem.

- `shadow_truth_fallback_total{site=resolve_price_context|resolve_gatekeeper_initial_reserves|tx_curve_enrichment_shadow}`
  Jawny licznik realnych wejsc w fallback ShadowLedgera na granicach truth/readiness.
  Na obecnym head po zamknieciu Fazy 4 nie ma juz tu `post_buy_price_read`; post-buy live lane nie ma prawa liczyc ShadowLedgera jako truth source.
  Licznik zawsze niesie tez label `category=bootstrap_only|degraded_diagnostic|hidden_primary`, wiec kazdy dozwolony site pozostaje jawnie sklasyfikowany.

- `degraded_truth_helper_total{site=...,helper=...,category=...}`
  Jawny licznik wszystkich faktycznie uzytych bounded helperow truth/readiness.
  Dla Fazy 4 obejmuje:
  - `resolve_price_context` + `helper=shadow_ledger_snapshot`
  - `resolve_gatekeeper_initial_reserves` + `helper=shadow_ledger_snapshot`
  - `tx_curve_enrichment_shadow` + `helper=shadow_ledger_curve`
  Po wycieciu RPC state-ingestu nie obejmuje juz helperow naprawczych opartych o RPC.

- `account_update_before_identity_total`
  Liczba AccountUpdate, ktore dotarly zanim runtime mial gotowa identity registration dla minta/poola.

- `seer.account_updates.received_total`
  Liczba AccountUpdate odebranych przez Seer przed parserem/resolve path.

- `seer.account_updates.before_mapping_total{store_outcome=inserted|replaced_newer|ignored_older}`
  Race-window baseline dla AccountUpdate, ktore trafily przed curve->mint mapping.
  To jest podstawowy sygnal, czy problem `before identity/before mapping` nadal wystepuje i jaki ma koszt.

- `seer.account_updates.pending_curve_replay_total`
  Liczba buffered AccountUpdate odtworzonych po rejestracji mappingu.

- `seer.account_updates.pending_curve_replay_dwell_ms`
  Czas czekania buffered AccountUpdate w race window przed replayem.

- `seer.account_updates.pending_curve_overwrite_total`
  Liczba przypadkow, gdy nowszy buffered AccountUpdate nadpisal starszy pending snapshot dla tego samego curve.

- `seer.account_updates.pending_curve_replay_send_failed_total`
  Krytyczny licznik failujacych replay sendow z pending queue.

- `seer.account_updates.pending_curve_parse_failed_total`
  Krytyczny licznik parse failure na buffered replay path.

- `account_update_build_none_total{reason=...}`
  Liczba przypadkow, w ktorych runtime nie byl w stanie zbudowac `AccountStateUpdate`.

- `account_update_promoted_from_bootstrap_total`
  Liczba pierwszych canonical update'ow, ktore promowaly pool z bootstrap state do canonical `AccountStateCore`.

## Gatekeeper

- `gatekeeper_buffer_size`
  Liczba aktywnych launcherowych bufferów commit/gatekeeper.
  Długotrwały wzrost bez commitów oznacza blokadę lub zalegający flow.

- `gatekeeper_verdict_latency_ms{outcome=...}`
  Czas od pierwszej obserwacji okna do terminalnego verdictu.

- `gatekeeper_buy_rate{outcome=...}`
  Licznik terminalnych outcome'ów Gatekeepera (`buy`, `reject`, `timeout`).

- `gatekeeper_pending_curve_total{reason=...}`
  Każde wejście Gatekeepera w Phase-5 path curve policy (`unknown_curve_*`, `stale_curve_*`).
  Używać do rozróżniania czy problemem jest brak curve truth, czy curve stale/finality downgrade.

- `gatekeeper_pending_curve_terminal_total{outcome=recovered|rejected|timed_out}`
  Dokładnie jeden terminal outcome `PendingCurve` na pool.
  - `recovered` — curve policy stała się actionable przed deadlinem
  - `rejected` — policy skonfigurowana na natychmiastowy reject
  - `timed_out` — oczekiwanie skończyło się po `curve_wait_ms`

- `canonical_first_update_latency_ms`
  Czas od otwarcia sesji obserwacyjnej do pierwszego canonical update w `AccountStateCore`.

- `timeout_without_canonical_updates_total`
  Liczba terminalnych timeoutow sesji, w ktorych nie pojawil sie ani jeden canonical update.

## Phase 3 / Phase 4 proof artifacts

- `logs/decisions.jsonl/coverages/coverage*.jsonl`
  To jest zrodlo prawdy dla coverage ratio w Fazie 3.
  Guardrail czyta header wypluwany przez `logs/decisions.jsonl/cov.py`:
  - `avg_coverage`,
  - `coverage_complete`,
  - `unresolved_count`,
  - `coverage_status_counts`.
  Domyslny wybor artefaktu ignoruje pliki `coverage*.partial.jsonl` oraz pomocnicze `coverage.jsonl` / `coverages_short*.jsonl`; proof-check ma pracowac na finalnym, timestampowanym JSONL z `cov.py`, nie na pliku przejsciowym ani skroconym.
  Ten artefakt odpowiada za pytanie "czy observed trade coverage jest wystarczajace".

- `logs/decisions.jsonl/seer_runtime_coverage_audit.jsonl`
  To jest operator-facing artifact proof dla zdrowia canonical ingest.
  Aby runtime-check mogl przejsc, artefakt musi byc schema v3 i niesc:
  - runtime-side `chain_truth -> runtime_accepted`,
  - `diagnostics.canonical_update_count`,
  - `diagnostics.canonical_first_update_latency_ms`,
  - `diagnostics.live_account_update_count`,
  - `diagnostics.timed_out_without_canonical_updates`,
  - race-window counters dla pending AccountUpdate replay.

- `python3 scripts/refactor_phase3_guardrails.py proof-check --contract configs/refactor/phase4_proof_gate.json --coverage-audit <path> --cov-output <path>`
  Repo-level acceptance gate dla formalnego zamkniecia Fazy 4 na obecnym head.
  Structural check pilnuje kontraktu kodu, braku regresji shadow-truth w post-buy oraz sklasyfikowanych helperow, a runtime check czyta dwa artefakty:
  - `cov.py` output dla coverage ratio,
  - `coverage_audit` dla canonical ingest health.
  Jesli potrzebujesz odtworzyc historyczny proof Fazy 3, podaj jawnie `--contract configs/refactor/phase3_proof_gate.json`.

- `trigger_buy_token_program_validation_total{override_present=true|false,proof_result=not_provided|matched|mismatched,source=canonical_mint_fetch|runtime_override_validated|canonical_mint_fetch_after_mismatch}`
  To jest runtime proof metric dla Fazy 4.
  Zlicza dla kazdego live BUY:
  - czy `account_overrides.token_program` w ogole byl obecny,
  - czy zgadzal sie z `mint_account.owner`,
  - z jakiego zrodla finalnie wzieto `token_program` do builda.
  Proof gate do wlaczenia conditional mint-fetch elision wymaga stalego okna z:
  - `override_present=true`,
  - `proof_result=mismatched` = `0`,
  - i odpowiednio duzej probki live BUY zgodnie z planem.

## Commit / Live Pipeline / Trigger

- `commit_loop_duration_ms`
  Czas jednego cyklu `GatekeeperCommitLoop`.

- `live_pipeline_flush_latency_ms`
  Czas pojedynczego `flush_ready()` loopa.

- `tx_send_latency_ms{transport=...}`
  Czas wysyłki tx mierzony od `decision_ts_ms`.
  `rpc_confirmed` oznacza ścieżkę RPC `send_and_confirm_transaction`.
  `jito_submit` oznacza czas do przyjęcia bundle submit, nie final chain confirmation.

- `tip_floor_cache_hit{result=hit|miss|skipped,source=...}`
  BUY-only licznik rozstrzygnięcia sender tip policy.
  Na aktywnym Helius Sender path po wyłączeniu dynamicznego `tip_floor` fetch oczekiwane jest
  głównie `result=skipped,source=sender_fixed_tip`: BUY używa stałego baseline tipu bez Jito HTTP.
  Historyczne `tip_floor_cache`, `jito_tip_floor` albo `stale_last_good` mogą pojawiać się tylko
  w starszych artefaktach albo w testach helperów.

- `tip_floor_cache_age_ms{source=...}`
  Wiek rekordu tip floor użytego przez BUY hot path.
  Dla `source=sender_fixed_tip` oczekiwane jest stale `0`, bo hot path nie korzysta z cache ani fetchu.

- `tip_floor_fetch_latency_ms{source=...}`
  Czas fetchu Jito tip floor dla BUY hot path.
  Dla `source=sender_fixed_tip` oczekiwane jest stale `0`, bo fetch został wyłączony z aktywnego
  Sender BUY path.

- `tip_floor_cache_mode_total{mode=...,source=...}`
  Rozkład trybu rozstrzygnięcia tip floor.
  Na aktywnym path oczekiwane jest głównie `mode=fixed_baseline,source=sender_fixed_tip`.
  Historyczne/helperowe mode pozostają:
  `fresh_cache`, `miss_refresh`, `stale_last_good`, `miss_refresh_failed`.
  Rzeczywisty wiek cache użytego przy dispatch nadal czytamy z
  `tip_floor_cache_age_ms`; advisory prewarm sam nie jest nowym source-of-truth.

- `tip_floor_inflight_join_total{result=...,source=...}`
  Czy hot path dolaczyl do juz trwajacego refreshu tip floor zamiast robic wlasny fetch.
  Na aktywnym path oczekiwane jest `result=disabled,source=sender_fixed_tip`, bo BUY nie probuje juz
  dolaczac do inflight refreshu tip floor.
  `result=joined` i `timed_out_fallback` pozostają znaczące tylko dla starszych artefaktów
  albo testów helperów.

- `tip_floor_inflight_wait_ms{result=...,source=...}`
  Jak dlugo BUY czekal na juz trwajacy inflight refresh tip floor.
  Dla `result=disabled,source=sender_fixed_tip` oczekiwane jest `0`.

- `priority_fee_cache_hit{result=hit|miss,source=...}`
  BUY-only licznik wyniku lookupu dynamic priority fee na ścieżce przygotowania BUY.
  `source=priority_fee_cache` oznacza świeży hit BUY-scoped cache.
  `source=helius_rpc` oznacza realny fetch, `source=fallback_*` oznacza wejście w fallback.

- `priority_fee_cache_age_ms{source=...}`
  Wiek wpisu priority fee użytego przez BUY hot path.
  W Fazie 0 baseline to `0`, bo cache jeszcze nie jest aktywny.

- `priority_fee_fetch_latency_ms{source=...}`
  Czas pobrania dynamic priority fee dla BUY hot path.

- `priority_fee_cache_mode_total{mode=...,source=...}`
  Rozkład trybu rozstrzygnięcia BUY priority fee.
  Po Fazie 1D oczekiwane mode:
  `fresh_cache`, `miss_refresh`, `miss_refresh_failed`.
  Po Fazie 6/6B wiek wpisu faktycznie użytego przez BUY nadal czytamy z
  `priority_fee_cache_age_ms`; advisory prewarm tylko przygotowuje cache.

- `priority_fee_inflight_join_total{result=...,source=...}`
  Czy BUY prepare path dolaczyl do juz trwajacego keyed refreshu priority fee.
  `joined` oznacza, ze current BUY skorzystal z RTM-started inflight refresh.
  `timed_out_fallback` oznacza bounded wait zakonczony powrotem do local fetch.

- `priority_fee_inflight_wait_ms{result=...,source=...}`
  Czas czekania na keyed inflight priority fee refresh.
  Wysoki `timed_out_fallback` oznacza, ze metadata-aware prewarm nie startuje wystarczajaco
  wczesnie albo klasa BUY jest zbyt pozno rozstrzygana.

- `trigger_buy_advisory_prewarm_total{kind=tip_floor|priority_fee,hook_phase=early|late,benefit_scope=...,result=...,cache_mode=...,source=...}`
  Telemetria advisory prewarm hooków wokół BUY hot path.
  `kind=tip_floor,hook_phase=early,benefit_scope=current_buy` pozostaje lekkim hookiem z
  `oracle_runtime`, ale przy `source=sender_fixed_tip` kończy się `result=skipped` i nie robi
  żadnego Jito `tip_floor` fetchu.
  `kind=priority_fee,hook_phase=late,benefit_scope=current_buy` to exact-key hook z
  `prepare_buy_request_with_tip_telemetry()`, uruchamiany dopiero po rozstrzygnięciu
  `token_program` i `ata_missing_pre_submit`, żeby prewarm i hot path używały tego samego
  `PriorityFeeCacheKey`.
  `kind=priority_fee,hook_phase=late,benefit_scope=current_or_next_buy` pozostaje
  telemetrycznym śladem best-effort probable-key warmupu, jeśli taki advisory path zostanie
  świadomie wywołany.
  `result=started` oznacza samo odpalenie fire-and-forget taska; `hit`/`miss`/`skipped`/`error`
  opisują zakończenie advisory path.

- `trigger_buy_advisory_prewarm_cache_age_ms{kind=...,hook_phase=...,benefit_scope=...,source=...}`
  Wiek cache entry obserwowany przez advisory prewarm w chwili jego zakończenia.
  Do oceny wieku użytego przy realnym BUY dispatch nadal preferujemy
  `tip_floor_cache_age_ms` albo `priority_fee_cache_age_ms` z właściwego hot path.

- `trigger_buy_advisory_prewarm_fetch_latency_ms{kind=...,hook_phase=...,benefit_scope=...,source=...}`
  Czas fetchu wykonanego przez advisory prewarm.
  Dla `kind=tip_floor,source=sender_fixed_tip` oczekiwane jest `0`, bo advisory path nie wykonuje
  już fetchu i tylko potwierdza politykę stałego baseline tipu.
  Duże wartości przy `miss` nie blokują BUY dispatch, ale pokazują czy prewarm realnie ma szansę
  dogrzać cache przed bieżącym `prepare_buy_request`. Dla
  `kind=priority_fee,hook_phase=late,benefit_scope=current_buy` oczekiwany dowód skuteczności
  to nie brak fetchu, tylko `trigger_buy_prewarm_join_total{kind="priority_fee",result="joined"}`
  zamiast drugiego niezależnego refreshu w BUY hot path.

- `trigger_buy_prewarm_join_total{kind=tip_floor|priority_fee,result=...}`
  BUY-path view na to, czy biezacy request rzeczywiscie dolaczyl do inflight prewarmu.
  Dla `kind=tip_floor` na aktywnym Sender path oczekiwane jest teraz `result=disabled`.
  Dla `kind=priority_fee` to nadal główny proof metric dla żywego prewarm/join path.

- `trigger_buy_prewarm_wait_ms{kind=tip_floor|priority_fee,result=...}`
  Czas czekania biezacego BUY na inflight prewarm.
  Dla `kind=tip_floor,result=disabled` oczekiwane jest `0`.
  Uzywac razem z `trigger_buy_advisory_prewarm_total`, zeby odroznic "prewarm byl"
  od "prewarm realnie pomogl current BUY".

- `payer_load_ms`
  Czas wczytania payer keypair w `TriggerComponent::prepare_buy_request()`.

- `payer_balance_fetch_ms`
  Czas RPC potrzebny do pobrania salda payera przed BUY.

- `payer_account_fetch_ms`
  Czas RPC potrzebny do pobrania konta payera przed BUY.

- `mint_account_fetch_ms`
  Czas RPC potrzebny do pobrania konta minta przed BUY.

- `token_balance_probe_ms`
  Czas potrzebny na logiczne rozstrzygnięcie `pre_submit_token_balance` i klasy `ATA path`.
  Po Fazie 2 fast path może zakończyć się jednym `getTokenAccountBalance`, ale wynik `ATA missing`
  nadal wymaga konserwatywnego fallbacku do probe o semantyce retry/secondary.

- `ata_rent_fetch_ms`
  Czas RPC potrzebny do pobrania rent exemption dla nowego ATA.
  `0` przy istniejącym ATA albo przy cache-hit rent oznacza poprawny, zmierzony skip RPC tej ścieżki,
  a nie brak pomiaru.

- Log `Trigger: BUY preparation breakdown` i `Trigger: prepared buy request accounts`
  niosą po Fazie 2 dwa jawne pola ATA:
  - `attach_idempotent_ata_create` — dla live BUY powinno być zawsze `true`
  - `ata_missing_pre_submit` — `true` tylko gdy brak ATA został potwierdzony konserwatywnym probe

- `build_once_ms`
  Czas od startu `create_buy_build_profile` do zakończenia pierwszego materializowanego
  builda BUY tx. Na cache-hit obejmuje profile creation + final build; na cache-miss
  profile creation + probe build, więc okno jest porównywalne między obiema ścieżkami.

- `rebuild_ms`
  Czas drugiego builda BUY tx po priority fee estimate albo rebuilda retry.

- `legacy_path_event_total{path=...}`
  Licznik wejść w jawnie sklasyfikowane legacy pathy Fazy 6.
  Oczekiwane ścieżki produkcyjne:
  - `trigger_pool_scored_observer` — legacy observation only
  - `trigger_embedded_oracle_pipeline` — compatibility-only surface
  - `trigger_no_event_bus_fallback` — disabled in production

- `legacy_path_side_effect_block_total{path=...}`
  Licznik zablokowanych prób przejścia legacy pathu do autorytatywnego BUY side effectu.
  Wzrost dla `trigger_pool_scored_observer` oznacza, że historyczna ścieżka scoringowa została poprawnie odcięta od canonical execution.

- Legacy terminal verdict path jest po PR8 ogrodzony do `#[cfg(test)]` i nie emituje juz production runtime metryki.

- `post_buy_price_source_total{source=canonical_account_state|rpc_point_query|unavailable}`
  Zrodlo ceny obserwowane przez live post-buy runtime.
  Po cutoverze Fazy 4 `ShadowLedger` nie jest juz legalnym source label dla live truth; pozostaje wylacznie compare-only.

- `post_buy_shadow_compare_total{primary_source=canonical_account_state|rpc_point_query,result=match|diverged|shadow_missing}`
  Compare-only licznik dual-read po BUY. Pokazuje, czy shadow snapshot zgadza sie z primary live source, ale nie ma prawa przejac roli truth source.

- `post_buy_shadow_compare_diff_bps{primary_source=canonical_account_state|rpc_point_query}`
  Histogram odchylenia compare-only pomiedzy primary live source a shadow snapshotem.

- `trigger_post_buy_handoff_failed_total{lane=...,transport=direct_queue|broadcast}`
  Terminalny brak handoffu `BUY -> PostBuyRuntime`.
  `transport=direct_queue` oznacza, ze padla autorytatywna, bezposrednia kolejka handoffu; to jest krytyczny sygnal fail-closed.
  `transport=broadcast` bez direct-queue sukcesu oznacza, ze nie bylo zadnej skutecznej drogi przekazania `PostBuySubmitted`.

- `trigger_post_buy_handoff_degraded_total{lane=...,transport=broadcast}`
  Direct handoff do `PostBuyRuntime` udal sie, ale broadcast-telemetria `PostBuySubmitted` nie zostala dowieziona po retrach.
  SELL lifecycle powinien nadal ruszyc; problem dotyczy wtornych konsumentow / obserwowalnosci.

- `post_buy_runtime_duplicate_handoff_total{lane=...}`
  Licznik zdedupowanych `PostBuySubmitted`.
  Oczekiwany wzrost po wdrozeniu direct handoff + broadcast mirror; chroni runtime przed podwojnym odpaleniem lifecycle dla tego samego BUY.

- `post_buy_live_sell_ata_resolution_failed_total`
  ATA po BUY nie byla widoczna nawet po bounded retry.
  To nadal oznacza terminalny fail-closed release pozycji i wymaga inspekcji RPC/account-state propagation.

- `post_buy_live_sell_magazine_load_failed_total`
  Live SELL magazynek nie uzbroil sie po bounded retry.
  BUY jest potwierdzony, ale runtime nie byl w stanie zbudowac presigned SELL bullets; traktowac jako krytyczny sygnal operacyjny.

- `post_buy_live_sell_bullet_corrupt_total{reason=empty_tx_bytes|deserialize_failed}`
  Jawnie wykryte uszkodzone bullets.
  `empty_tx_bytes` oznacza pusty payload, `deserialize_failed` oznacza nieparsowalne serialized tx bytes.

- `post_buy_live_sell_bullet_quarantined_total{reason=empty_tx_bytes|deserialize_failed|requeue_exhausted}`
  Bullet zostal odciety od aktywnego fire-setu i przeniesiony do quarantine.
  Lifecycle pozostaje fail-closed/sticky — pozycja nie jest uznawana za zamknieta, dopoki operator nie zareaguje albo nie pojawia sie inne aktywne bullets.

- `post_buy_live_sell_stale_bullet_total`
  Liczba SELL bullets, ktore weszly w submit path jako stale i wymagaly refresh blockhashu przed Jito submit.
  Rosnacy trend oznacza, ze price loop trafia na coraz starsze bullets lub transport jest zbyt wolny wzgledem TTL.

## Log planes Fazy 6

W logach hot path powinny być teraz rozróżnialne cztery execution planes:

- `runtime_plane=canonical_decision`
  Autorytatywne decyzje runtime prowadzące do `TransactionSent` / `PostBuySubmitted`.

- `runtime_plane=legacy_observation`
  Historyczne/kompatybilnościowe ścieżki (`PoolScored`, embedded `OraclePipeline`, fallback bez event busa), które nie mają prawa emitować realnego BUY.

- `runtime_plane=shadow_simulation`
  Compare-only shadow execution i jego telemetria.

- `runtime_plane=post_buy_monitoring`
  Monitoring po BUY w `PostBuyRuntime`; nie jest to execution plane dla nowych BUY side effectów.
