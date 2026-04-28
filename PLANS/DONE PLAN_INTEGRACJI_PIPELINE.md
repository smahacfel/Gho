# PLAN WYKONAWCZY v2: Ghost Pipeline → Production-Safe, potem Competitive Edge

**Data:** 2026-03-18  
**Cel:** doprowadzenie pipeline najpierw do stanu **production-safe**, a następnie do stanu **competitive / scale-ready**, bez dublowania mechanizmów, które już istnieją w repo.  
**Metoda:** fazy priorytetowe (P0→P3), każde zadanie ma: aktualny stan kodu, zakres zmiany, kryterium akceptacji i test.

---

## ZAŁOŻENIA KORYGUJĄCE PLAN

Ten plan uwzględnia aktualny stan repo na dzień 2026-03-18:

1. `dev_buy_sol` / `has_dev_buy` są nadal błędnie hardcodowane w launcherze — to pozostaje **krytycznym bugiem**.
2. `PendingCurve` / curve latch już istnieją (`curve_wait_ms`, `curve_t0_event_ts_ms`, `curve_wait_elapsed_ms`) — zadanie dotyczy **audytu i domknięcia istniejącego flow**, a nie budowy drugiego timeoutu od zera.
3. Shared WAL jest już częściowo wpięty w Seer i OracleRuntime (`RawTx`, `ParsedEvent`, `Decision`) — zadanie dotyczy **rozszerzenia recovery semantics**, nie „pierwszej integracji”.
4. Freshness enrichmentu nie jest już czystym hardcodem — istnieje env override; celem jest **przeniesienie do SSOT configu i ujednolicenie semantyki fresh/stale/unknown**.
5. `ReconciliationRuntime` już istnieje — zadanie dotyczy **hardeningu, alertingu i metryk**, a nie tworzenia nowego runtime.

---

## DWUETAPOWA DEFINICJA CELU

### Etap A — Production-Safe Baseline

Pipeline jest gotowy do realnej pracy operacyjnej, gdy:

1. ✅ Gatekeeper widzi prawdziwe `dev_buy_sol` i `has_dev_buy`
2. ✅ `PendingCurve` kończy się w przewidywalny sposób i ma pełną telemetrykę
3. ✅ WAL + snapshot pozwalają odzyskać stan po restarcie bez utraty krytycznego flow
4. ✅ freshness SLA i fallback stale/unknown są spójne i konfigurowalne z jednego miejsca
5. ✅ reconciliation ma alerty driftu i nie failuje po RPC errors
6. ✅ hot-path telemetry pokrywa must-have operacyjne

### Etap B — Competitive / Scale Edge

Pipeline ma przewagę wykonawczą i skalowalność, gdy dodatkowo:

1. ✅ transport ma circuit-breaker i policzalny stall-rate
2. ✅ event bus skaluje się przez sharding / routing
3. ✅ execution używa adaptive fee + multi-endpoint send
4. ✅ ShadowLedger ma semantykę finality i kontrolowany rollback
5. ✅ istnieją chaos / soak / replay-equivalence testy systemowe

---

## FAZA 0 — KRYTYCZNE BUGI POPRAWNOŚCI (blokują jakość decyzji)

### Z0.1 — Fix `dev_buy_sol` / `has_dev_buy` hardcode

**Stan repo:** bug realnie istnieje w launcherze i wpływa na Gatekeeper / scoring.  
**Problem:** `build_enhanced_candidate_from_pool_data()` i `convert_to_enhanced_candidate()` hardcodują `dev_buy_sol: 0.0`, `has_dev_buy: false`. Gatekeeper V2 i downstream scoring nie widzą realnego dev buy.

**Pliki do zmiany:**
- `ghost-launcher/src/oracle_runtime.rs` → `build_enhanced_candidate_from_pool_data()`
- `ghost-launcher/src/components/oracle_pipeline.rs` → `convert_to_enhanced_candidate()`
- opcjonalnie wspólny helper, jeśli lookup ma być współdzielony

**Implementacja:**

1. W `build_enhanced_candidate_from_pool_data()`:
   - użyć `DetectedPool.creator` jako źródła dev identity,
   - zidentyfikować pierwszy BUY od creatora w pierwszych N tx poola,
   - ustawić:
     - `dev_buy_sol = tx.volume_sol`
     - `has_dev_buy = true`
   - jeśli brak dopasowania → `dev_buy_sol = 0.0`, `has_dev_buy = false`.

2. W `convert_to_enhanced_candidate()`:
   - zastosować tę samą logikę,
   - nie duplikować heurystyk w dwóch miejscach, jeśli można wyciągnąć helper.

3. Zachować guard z pathu Seer:
   - propagować wynik kompatybilnie z `sanitize_dev_buy()`,
   - zachować clamp / NaN protection / negative protection.

4. Zweryfikować propagację do:
   - `EnhancedCandidate`,
   - Gatekeeper / Phase 5,
   - logowania JSONL / buy logów,
   - downstream scoringów wykorzystujących `has_dev_buy` / `dev_buy_sol`.

**Kryterium akceptacji:**
- [ ] `dev_buy_sol > 0.0` i `has_dev_buy = true`, gdy creator kupuje we wczesnych tx
- [ ] `dev_buy_sol == 0.0` i `has_dev_buy = false`, gdy creator nie kupuje
- [ ] values docierają do JSONL / decision logów
- [ ] test: mock pool z dev buy → candidate ma prawidłowe pola
- [ ] test: mock pool bez dev buy → fallback 0/false
- [ ] sanitize guard zachowany

**Szacunek:** 4-6h

---

### Z0.2 — Audit + hardening istniejącego `PendingCurve` / curve latch

**Stan repo:** mechanizm istnieje (`curve_wait_ms`, `set_curve_t0()`, `curve_wait_elapsed_ms`, `PendingCurve`, hard reject po deadline).  
**Problem:** trzeba potwierdzić, że istniejący latch zachowuje się poprawnie operacyjnie dla stale/unknown curve data i że nie kończy się „cichym” długim niedookreślonym waitingiem.

**Pliki do zmiany:**
- `ghost-launcher/src/oracle_runtime.rs` → `enrich_pool_tx_from_shadow_ledger()`
- `ghost-launcher/src/components/gatekeeper.rs` → istniejący handling `PendingCurve`
- `ghost_brain_config.toml` / odpowiadający config source dla curve wait, jeśli potrzebna korekta SSOT

**Implementacja:**

1. W `enrich_pool_tx_from_shadow_ledger()`:
   - dodać jawne metryki dla ścieżki `unknown/stale → PendingCurve`, np.:
     - `shadow_ledger_enrichment_pending_curve_total`
     - histogram wieku snapshotu dla hot pathu,
   - odróżnić telemetrycznie:
     - `fresh`
     - `stale`
     - `unknown`.

2. W `GatekeeperBuffer` / curve latch:
   - **nie dodawać od razu drugiego timeoutu typu `curve_wait_budget_ms`**, jeśli obecny `curve_wait_ms` może pełnić tę rolę,
   - najpierw zweryfikować semantykę istniejącego deadline:
     - czy `PendingCurve` kończy się deterministycznym rejectem,
     - czy reason i telemetry są poprawne,
     - czy długi brak AccountUpdate nie kończy się mylącym `Timeout`, jeśli powinien być curve-specific reject.

3. Jeśli wynik audytu pokaże lukę:
   - rozszerzyć **istniejący** latch / reason chain,
   - preferować reuse `curve_wait_ms` zamiast wprowadzać drugi niemal-identyczny licznik.

4. Telemetria:
   - upewnić się, że `curve_t0_event_ts_ms` i `curve_wait_elapsed_ms` są zawsze wypełnione na ścieżkach `PendingCurve` / reject.

### Doprecyzowanie po samodzielnym audycie repo

Audyt kodu na 2026-03-18 potwierdza, że obecny hardening zamyka większość luki Z0.2, ale zostają jeszcze 3 punkty, które warto dopisać jawnie, żeby „DONE” nie było zbyt optymistyczne:

1. **Brak pełnego testu scenariusza recovery-before-deadline:**
   - istnieją testy `PendingCurve -> Reject po deadline` oraz `curve known od początku -> normalny flow`,
   - brakuje testu dokładnie dla ścieżki:
     - `genesis_seed / curve_data_known=false` na wejściu,
     - potem `PendingCurve`,
     - następnie `AccountUpdate` / świeże `curve_data_known=true` **przed** deadline,
     - i dopiero wtedy normalna ocena / BUY lub zwykły verdict bez curve-specific reject.

2. **Brak testu integracyjnego fallbacku t0 w runtime:**
   - `pool_observation_task()` ustawia `curve_t0` sensownie (`pool_data.timestamp_ms` albo fallback `registered_wall_ts_ms`),
   - ale brakuje testu dowodzącego, że ścieżka z opóźnionym / późno dostarczonym `NewPoolDetected` nie psuje semantyki latcha.

3. **JSONL nie niesie jeszcze pełnego kontekstu t0:**
   - `curve_wait_elapsed_ms` jest już serializowane do `GatekeeperBuyLog`,
   - ale `curve_t0_event_ts_ms` pozostaje tylko w `GatekeeperAssessment` i nie trafia do JSONL,
   - jeśli chcemy pełnej post-mortem telemetryki curve-latch w decision logach, pole `curve_t0_event_ts_ms` trzeba dopisać do schematu logu.

**Kryterium akceptacji:**
- [ ] `shadow_ledger_enrichment_pending_curve_total` jest emitowana
- [ ] `curve_wait_elapsed_ms` w JSONL odpowiada realnemu waitingowi
- [ ] test: genesis seed + brak AccountUpdate → `PendingCurve` → curve-specific terminal reject po deadline
- [ ] test: genesis seed + AccountUpdate przed deadline → normalny buy / normalna ocena
- [ ] test: fallback `registered_wall_ts_ms` + późny `NewPoolDetected` nie psują semantyki `curve_t0`
- [ ] jeśli wymagamy pełnej telemetryki post-mortem: `curve_t0_event_ts_ms` trafia także do JSONL / decision logów
- [ ] brak równoległego, dublującego timeoutu bez uzasadnienia architektonicznego

**Szacunek:** 3-5h

---

## FAZA 1 — DURABILITY: WAL + SNAPSHOT + RECOVERY

### Z1.1 — Rozszerzenie istniejącego shared WAL do pełnego recovery flow

**Stan repo:** shared WAL już istnieje i zapisuje co najmniej `RawTx`, `ParsedEvent`, `Decision`; Seer i OracleRuntime są już częściowo podpięte.  
**Problem:** obecny WAL nie pokrywa jeszcze pełnej semantyki recovery dla commit/live/shadow-ledger state po restarcie.

**Pliki do zmiany:**
- `ghost-core/src/wal.rs` → rozszerzenie `WalRecord`
- `off-chain/components/seer/src/lib.rs` → nowe appendy dla recovery-critical eventów
- `ghost-launcher/src/oracle_runtime.rs` → rozszerzenie decision/recovery records
- `ghost-launcher/src/main.rs` → startup replay
- `ghost-launcher/src/components/gatekeeper.rs` lub commit coordinator → commit-related records

**Implementacja:**

1. Rozszerzyć `WalRecord` o recovery-critical rekordy, np.:
   - `TradeForwarded`
   - `CommitStaged`
   - `CommitPersisted`
   - `ShadowLedgerCurveUpdate`

2. Zachować obecną integrację i **rozszerzyć** ją o:
   - forward do live path,
   - commit staging/persist,
   - curve updates wykorzystywane do odbudowy stanu.

3. Startup replay:
   - zdefiniować jawne zasady odtwarzania:
     - commit staged bez persisted → re-stage,
     - curve update → restore delta state,
     - trade forwarded po ostatnim persisted → pending_live rebuild.

4. Konfiguracja:
   - docelowo jedno źródło prawdy dla WAL configu,
   - jeśli dziś używany jest env (`GHOST_WAL_DIR`, itd.), plan migracji do configu ma być jawny, a nie „ukryty side-effect”.

5. Observability:
   - `wal_append_latency_us`
   - `wal_replay_records_total`
   - `wal_segment_rotation_total`

**Kryterium akceptacji:**
- [ ] shared WAL nadal działa dla istniejących recordów
- [ ] nowe recordy pozwalają odbudować commit/live/shadow-ledger delta po restarcie
- [ ] restart procesu → staged commits są odzyskiwane i finalizowane
- [ ] restart procesu → curve updates są odzyskiwane do stanu runtime
- [ ] test: append/replay 10K records
- [ ] test: truncated tail tolerowane
- [ ] test: retention purge działa

**Szacunek:** 10-14h

---

### Z1.2 — ShadowLedger periodic snapshot do dysku

**Stan repo:** brak compact snapshot recovery; restart opiera się za bardzo na replay.  
**Problem:** sam WAL nie wystarczy do szybkiego restartu przy dużym stanie.

**Pliki do zmiany:**
- `ghost-core/src/shadow_ledger/ledger.rs`
- `ghost-core/src/shadow_ledger/storage.rs`
- `ghost-launcher/src/main.rs`

**Implementacja:**

1. `ShadowLedger::snapshot_to_disk(path)`:
   - serializacja curves + snapshot_commit_state + powiązań indeksów,
   - atomowy zapis `.tmp -> rename()`,
   - metryki write time / curves count.

2. `ShadowLedger::restore_from_disk(path)`:
   - restore najnowszego poprawnego snapshotu,
   - walidacja odrzucająca oczywiście nieużyteczne / bootstrap-only entry (`slot=0`, jeśli to nadal właściwa semantyka),
   - metryki restore time / curves count.

3. Startup:
   - restore snapshot,
   - potem WAL replay tylko jako delta od snapshotu.

4. Rotacja:
   - utrzymywać max N ostatnich snapshotów (np. 3),
   - awaria podczas zapisu nie może psuć poprzedniego snapshotu.

**Kryterium akceptacji:**
- [ ] snapshot pojawia się cyklicznie
- [ ] restart → snapshot restore + WAL replay daje ten sam stan logiczny
- [ ] test: serialize/deserialize roundtrip dla 1000 curves
- [ ] test: crash mid-write nie niszczy ostatniego poprawnego snapshotu
- [ ] startup time dla dużego stanu jest wyraźnie lepszy niż full replay-only

**Szacunek:** 8-10h

---

## FAZA 2 — KONFIGURACJA, FRESHNESS, RECONCILIATION, METRYKI

### Z2.1 — Przeniesienie freshness SLA do jednego SSOT configu

**Stan repo:** istnieje default `200ms` i env override dla enrichment freshness; brak spójnej semantyki w głównym configu.  
**Problem:** freshness policy jest częściowo konfigurowalna, ale nie stanowi spójnego modelu operacyjnego.

**Pliki do zmiany:**
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost_brain_config.toml` lub docelowy wspólny config SSOT
- `ghost-core/src/shadow_ledger/ledger.rs`

**Implementacja:**

1. Dodać sekcję konfiguracyjną, np.:

```toml
[shadow_ledger]
enrichment_freshness_ms = 200
stale_fallback = "pending_curve"  # pending_curve | use_stale_with_warning | reject
```

2. Zmienić runtime tak, aby:
   - default mógł pozostać, ale był wtórny wobec configu,
   - env override miało jasno opisaną semantykę (tymczasowy override albo legacy),
   - decyzja o `fresh/stale/unknown` była jawna i wspólna.

3. W `ledger.rs` / helperze:
   - udostępnić helper zwracający freshness state,
   - nie ograniczać semantyki tylko do boola „curve known”.

4. Dodać metryki:
   - `shadow_ledger_curve_freshness_state{state=...}`
   - histogram wieku snapshotów.

**Kryterium akceptacji:**
- [ ] config freshness jest czytany z jednego SSOT miejsca
- [ ] po restarcie procesu zmiana w TOML wpływa na runtime
- [ ] `stale_fallback = "reject"` daje jawny reject zamiast `PendingCurve`
- [ ] test: stale curve → poprawna klasyfikacja stale + fallback
- [ ] test: fresh curve → normal enrichment

**Szacunek:** 3-4h

---

### Z2.2 — Provider circuit-breaker + stall-rate

**Stan repo:** transport ma watchdog stall detection, bounded queues i observability, ale brak jawnego circuit-breakera.  
**Problem:** system wykrywa stall, ale nie ma pełnej polityki odcinania wadliwego providera i kontrolowanego powrotu.

**Pliki do zmiany:**
- `off-chain/components/seer/src/grpc_connection.rs`

**Implementacja:**

1. Dodać `ProviderCircuitBreaker`:
   - stany `Closed / Open / HalfOpen`,
   - `max_stalls_before_open`,
   - `cooldown_ms`,
   - probe po cooldownie.

2. W connection loop:
   - stall/reconnect zwiększa licznik,
   - `Open` wyłącza providera z normalnego toru,
   - `HalfOpen` robi kontrolowany probe.

3. Metryki:
   - `ghost.pump.provider_stall_total{provider=...}`
   - `ghost.pump.provider_state{provider=...}`
   - `ghost.pump.stall_rate`

**Kryterium akceptacji:**
- [ ] po N stallach provider przechodzi do `Open`
- [ ] po cooldownie `HalfOpen` robi probe i może wrócić do `Closed`
- [ ] test: mock provider stale stallujący otwiera circuit
- [ ] stall-rate widoczny w Prometheus

**Szacunek:** 4-6h

---

### Z2.3 — Kompletne metryki hot-path (must-have operacyjne)

**Stan repo:** telemetryka jest szeroka, ale niepełna i niespójna nazewniczo.  
**Problem:** operator widzi dużo, ale nie ma jednego kompletnego zestawu must-have.

**Pliki do zmiany:** wiele modułów

**Metryki do domknięcia / ujednolicenia:**

| Metryka | Plik | Typ | Uwagi |
|---|---|---|---|
| `ingestion_latency_ms` | `grpc_connection.rs` | histogram | recv → parser output |
| `parser_malformed_tx_rate` | `binary_parser.rs` | counter/gauge | parser failure ratio |
| `eventbus_lag_total` | `oracle_runtime.rs` | counter | suma `RecvError::Lagged` |
| `eventbus_active_receivers` | `events.rs` | gauge | receiver_count |
| `shadow_ledger_age_ms` | `ledger.rs` | histogram | globalny age distribution |
| `enrichment_latency_ms` | `oracle_runtime.rs` | histogram | potwierdzić i nazewniczo spiąć z istniejącą metryką |
| `gatekeeper_buffer_size` | `gatekeeper.rs` | gauge | aktywne bufory / tx count |
| `gatekeeper_verdict_latency_ms` | `oracle_runtime.rs` | histogram | first tx → verdict |
| `gatekeeper_buy_rate` | `oracle_runtime.rs` | counter/gauge | buy vs terminal outcomes |
| `commit_loop_duration_ms` | `gatekeeper_commit_loop.rs` | histogram | czas cyklu |
| `tx_send_latency_ms` | `trigger/component.rs` | histogram | trigger → confirmation |
| `live_pipeline_flush_latency_ms` | live pipeline module | histogram | `flush_ready()` latency |

**Kryterium akceptacji:**
- [ ] wszystkie must-have metryki są emitowane
- [ ] istniejąca telemetryka nie jest dublowana niepotrzebnie pod trzema różnymi nazwami
- [ ] dashboard / runbook opisuje interpretację każdej metryki
- [ ] test/integration smoke potwierdza, że metryki zmieniają się podczas pracy

**Szacunek:** 6-8h

---

### Z2.4 — Hardening istniejącego `ReconciliationRuntime`

**Stan repo:** `ReconciliationRuntime` już istnieje i obsługuje reconciliation/observability.  
**Problem:** brakuje progu alarmowego driftu i pełnych metryk operacyjnych dla RPC/cycle.

**Pliki do zmiany:**
- `ghost-core/src/shadow_ledger/reconciliation_runtime.rs`
- `ghost-core/src/shadow_ledger/reconciliation.rs` (jeśli potrzebne)
- `ghost-launcher/src/oracle_runtime.rs`

**Implementacja:**

1. Dodać:
   - `shadow_ledger_reconciliation_drift_lamports` — histogram
   - `shadow_ledger_reconciliation_critical_drift_total` — counter
   - `shadow_ledger_reconciliation_cycle_ms` — histogram
   - `shadow_ledger_reconciliation_rpc_errors_total` — counter

2. Alert threshold:
   - jeśli drift > `drift_threshold_lamports`, emit WARN + counter,
   - nie crashować runtime po RPC errors.

3. Konfiguracja:
   - drift threshold ma być jawnie konfigurowalny,
   - nie ukryty jako magic number.

**Kryterium akceptacji:**
- [ ] drift > threshold → WARN + counter
- [ ] RPC failure → error counter, bez crasha
- [ ] test: drifted curve → reconciliation naprawia i emituje telemetrykę

**Szacunek:** 3-4h

---

## FAZA 3 — SKALOWALNOŚĆ I EXECUTION EDGE

### Z3.1 — Opcjonalny sharded fan-out dla hot path

**Stan repo:** obecny broadcast działa, ale będzie ograniczeniem przy wzroście throughputu.  
**Problem:** globalny broadcast powoduje, że każdy konsument dostaje każdy event.

**Pliki do zmiany:**
- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/oracle_runtime.rs`

**Implementacja:**

1. Dodać tryb `sharded`, zachowując `broadcast` jako domyślny fallback.
2. Routing pool transaction po hash(pool_id).
3. Non-pool events zostają na torze globalnym.
4. Dodać depth metrics per shard.

**Kryterium akceptacji:**
- [ ] tryb `broadcast` pozostaje kompatybilny
- [ ] tryb `sharded` redukuje lag pod wysokim loadem
- [ ] test: multi-pool delivery bez gubienia eventów

**Szacunek:** 10-14h

---

### Z3.2 — Adaptive fee bidding + multi-endpoint send

**Stan repo:** trigger ma już retry / dual-RPC / Jito primitives, ale bez adaptacyjnego biddingu.  
**Problem:** execution nie wykorzystuje dynamicznego fee pressure i nie używa pełnego równoległego wyścigu endpointów.

**Pliki do zmiany:**
- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-launcher/src/components/trigger/jito_tip.rs`

**Implementacja:**

1. `FeeOracle`:
   - recent fee window,
   - `p50` / `p90`,
   - adaptive tip z hard cap.

2. Multi-endpoint send:
   - równoległy send do Jito + RPC endpoints,
   - **pierwszy sukces wygrywa**, a nie „pełne anulowanie” wszystkich już wysłanych requestów,
   - pozostałe wyniki są ignorowane / kończone best-effort.

3. Metryki:
   - endpoint latency,
   - tip amount,
   - winner endpoint.

**Kryterium akceptacji:**
- [ ] adaptive tip rośnie wraz z fee pressure
- [ ] tx jest wysyłany równolegle do ≥2 endpointów
- [ ] test: mock endpoints o różnej latencji → najszybszy sukces wygrywa
- [ ] safe tip guard nadal obowiązuje

**Szacunek:** 10-14h

---

### Z3.3 — Finality tiers, a rollback jako osobny pod-etap

**Stan repo:** brak formalnej semantyki finality w ShadowLedger path.  
**Problem:** system naprawia drift, ale nie modeluje jakości / finality stanu, na którym zapada decyzja.

**Pliki do zmiany:**
- `ghost-core/src/shadow_ledger/ledger.rs`
- `ghost-core/src/shadow_ledger/history_types.rs`

**Implementacja — etap 1 (must-have):**

1. Dodać `CurveFinality`:
   - `Speculative`
   - `Provisional`
   - `Finalized`

2. Propagować finality do:
   - ShadowLedger state,
   - Gatekeeper assessment / buy log,
   - Phase 6 penalties / caution flags.

**Implementacja — etap 2 (osobny scope):**

3. Rollback / re-evaluation:
   - przy divergence przywrócić on-chain truth,
   - jawnie zdefiniować, które pending decisions mogą być re-evaluated,
   - metryka `shadow_ledger_rollback_total`.

**Kryterium akceptacji:**
- [ ] etap 1: finality propagowane end-to-end
- [ ] test: genesis → `Speculative` → AccountUpdate → `Provisional`
- [ ] rollback nie jest „wrzucony przy okazji”, tylko ma osobny zakres i testy

**Szacunek:**
- etap 1: 6-10h
- etap 2: 10-16h

---

### Z3.4 — Chaos + property testy systemowe

**Stan repo:** test coverage istnieje, ale brakuje chaos/permutation/replay-equivalence suite.  
**Problem:** brakuje dowodu odporności na ordering, failover, replay i długie obciążenie.

**Pliki do zmiany:**
- nowy katalog testów systemowych / chaos
- harness integracyjny w launcherze / core

**Scenariusze:**

1. Ordering permutation test
2. Provider failover test
3. Reorg simulation
4. High-throughput soak
5. Replay equivalence

**Kryterium akceptacji:**
- [ ] determinism / failover / replay tests są częścią regularnej integracji
- [ ] ciężki soak test może być uruchamiany jako nightly / perf job, niekoniecznie jako obowiązkowy szybki CI gate
- [ ] documented exceptions dla niedeterministycznych edge cases są jawne

**Szacunek:** 16-24h

---

## KOLEJNOŚĆ REALIZACJI

### Sprint 1 — Production-Safe baseline

1. `Z0.1` — dev buy fix
2. `Z0.2` — curve latch audit + hardening
3. `Z1.1` — WAL recovery extension
4. `Z1.2` — ShadowLedger snapshot
5. `Z2.1` — freshness SSOT
6. `Z2.4` — reconciliation hardening
7. `Z2.3` — must-have telemetry

### Sprint 2 — Resilience + scale

1. `Z2.2` — circuit breaker
2. `Z3.1` — sharded bus
3. `Z3.2` — adaptive fee + multi-endpoint send

### Sprint 3 — Correctness edge + system proof

1. `Z3.3` etap 1 — finality tiers
2. `Z3.3` etap 2 — rollback / re-evaluation
3. `Z3.4` — chaos + replay + soak

---

## SZACUNEK CZASU (PO KOREKCIE O TO, CO JUŻ ISTNIEJE)

```text
FAZA 0:  Z0.1 + Z0.2                           =  7-11h
FAZA 1:  Z1.1 + Z1.2                           = 18-24h
FAZA 2:  Z2.1 + Z2.2 + Z2.3 + Z2.4            = 16-22h
FAZA 3:  Z3.1 + Z3.2 + Z3.3 + Z3.4            = 42-64h
                                             ----------------
TOTAL                                          = 83-121h
```

To nadal jest duży zakres, ale uczciwiej odzwierciedla fakt, że część infrastruktury już istnieje i wymaga rozszerzenia, a nie budowy od zera.

---

## DEFINICJA DONE

Plan jest wykonany poprawnie dopiero wtedy, gdy:

### Done dla baseline

- [ ] dev-buy nie jest hardcodowany
- [ ] curve latch ma poprawną semantykę i telemetrykę
- [ ] restart odtwarza runtime state z WAL + snapshot
- [ ] freshness policy ma jedno źródło prawdy
- [ ] reconciliation emituje alerty driftu i znosi RPC errors
- [ ] operator ma pełen zestaw must-have metryk

### Done dla edge

- [ ] provider failover jest kontrolowany circuit-breakerem
- [ ] event bus skaluje się w trybie sharded
- [ ] execution używa adaptive fee i parallel endpoint race
- [ ] finality jest jawna, a rollback ma własne testy
- [ ] testy chaos / replay / soak dowodzą odporności systemu

---

## NOTATKA ARCHITEKTONICZNA

Przy każdej zmianie obowiązuje zasada:

> **Najpierw rozszerzamy i domykamy istniejące mechanizmy, dopiero potem dokładamy nowe byty konfiguracyjne lub nowe timeouty.**

To chroni repo przed dublowaniem:
- dwóch timeoutów dla curve wait,
- dwóch źródeł prawdy dla freshness,
- dwóch narracji o tym, czy WAL / reconciliation „już istnieją”, czy nie.