# AUDYT PIPELINE'U GATEKEEPERA V2.5

Pełny opis aktywnego flow Ghost: ingest danych o poolach i tokenach,
materializacja dowodów decyzyjnych, ewaluacja Gatekeepera V2/V2.5,
wydanie decyzji, shadow-burnin, logowanie i replay.

Data aktualizacji: 2026-05-13

Repo: `/root/Gho`

HEAD użyty do audytu: `78ef5a4d77d5d92d66361ec8f85480908371069b`

Główny profil walidacyjny: `configs/rollout/shadow-burnin.toml`

Główny config decyzyjny: `ghost-brain/ghost_brain_config.toml`

Uwaga o stanie roboczym: podczas audytu working tree miał zastaną
modyfikację w `ghost-brain/ghost_brain_config.toml`: `min_market_cap_sol`
w `[gatekeeper_v2.phase6_curve]` wynosiło `41.0`, podczas gdy czysty `HEAD`
miał `60.0`. Opis niżej traktuje roboczy checkout jako stan aktualny, bo to
on jest źródłem bieżącej konfiguracji w tej sesji.

## 0. Zakres i wyłączenia

Ten dokument opisuje aktywny pipeline Gatekeepera V2.5 od wejścia danych
łańcuchowych do terminalnej decyzji i shadow execution evidence. Jest napisany
dla eksperta, który ma oceniać, czy logika biznesowa oceny pooli/tokenów ma
sens i gdzie należy ją dalej rozwijać.

W zakresie są:

- Seer / Yellowstone / gRPC ingest i bridge do `GhostEvent`.
- `EventBus`, routing w `OracleRuntime`, per-pool observation task.
- `PoolObservationSession`, `AccountStateCore`, `TxIntelligenceEngine`,
  `CheckpointEngine`, CPV/FSC i early fingerprinting.
- `MaterializedFeatureSet` jako kanoniczny snapshot decyzyjny.
- Gatekeeper V2 core phases, hard filters, soft policies i curve gate.
- V2.5: DOW, TAS, PDD, APS, typed reason codes, confidence i shadow decisions.
- IWIM jako post-Gatekeeper BUY veto przed wykonaniem.
- Shadow-burnin: konfiguracja, shadow-only trigger, symulacja buy,
  lifecycle JSONL, post-buy shadow monitor i raportowanie.
- DecisionLogger, schema v19, plane routing, replay/report gates.

Poza zakresem aktywnego runtime są legacy relikty:

- HyperOracle / HyperPrediction / Chaos w roli aktywnych źródeł decyzji.
- `OracleRuntime::score_pool()` w roli produkcyjnego scorera.
- `GhostEvent::PoolScored` w roli aktualnego toru decyzyjnego.
- Test-only/compat helpers, np. `evaluate_compat_from_features()` i
  `PoolObservationSession::legacy_test_verdict_from_transaction()`.

Te elementy mogą istnieć w kodzie, ale nie są źródłem prawdy dla obecnego
Gatekeepera V2.5.

## 1. Jednozdaniowy model systemu

Ghost nie jest ogólnym predyktorem. Ghost jest selektywnym runtime, który
obserwuje świeży pool w krótkim oknie, materializuje jeden kanoniczny zestaw
dowodów (`MaterializedFeatureSet`), odpala deterministyczną politykę
Gatekeepera, a następnie w profilu shadow-burnin symuluje, co zrobiłby live
execution path bez wysyłania transakcji na łańcuch.

Najważniejszy kontrakt:

```text
Seer / Yellowstone
  -> GhostEvent EventBus
  -> OracleRuntime
  -> PoolObservationSession
  -> MaterializedFeatureSet
  -> Gatekeeper V2/V2.5 policy
  -> IWIM veto
  -> shadow/live trigger handoff
  -> DecisionLogger / shadow lifecycle / replay reports
```

## 2. Non-negotiable contracts

Te reguły są ważniejsze niż wygoda implementacyjna:

- `MaterializedFeatureSet` jest jedynym kanonicznym snapshotem decyzyjnym.
- Gatekeeper policy nie może recompute'ować autorytatywnych feature'ów z
  konkurencyjnych mutable źródeł.
- Active terminal evaluation idzie przez feature-driven path:
  `materialize_features()` -> `evaluate_from_features()`.
- Każdy terminalny verdict musi mieć typ i reason code.
- Generic `REJECT` nie jest wystarczającym dowodem audytowym, jeśli istnieje
  bardziej szczegółowy typ.
- Hard safety filters mają pierwszeństwo przed soft score.
- Shadow simulation nie jest live inclusion.
- Submit nie jest confirmation.
- Unknown execution status nie jest success.
- `no_dispatch_*` po reject/timeout nie jest błędem shadow lifecycle.
- Legacy/test-only ścieżki nie mogą być reaktywowane przypadkiem.
- Config fields wpływające na decyzje muszą być config-driven i kompatybilne
  wstecznie (`serde(default)` tam, gdzie stare configi mają się ładować).

## 3. Aktualny profil operacyjny

### 3.1 Gatekeeper V2 core

Źródło: `ghost-brain/ghost_brain_config.toml`.

Aktualny tryb Gatekeepera:

- `[gatekeeper_v2].enabled = true`
- `[gatekeeper_v2].mode = "long"`
- `max_wait_time_ms = 10000` w `phase1_quantity`
- `min_phases_to_pass = 4`
- `curve_require_for_buy = true`
- `curve_wait_ms = 500`
- shadow/V2.5 włączony, live execution V2.5 wyłączony.

Core phase thresholds w aktualnym checkoutcie:

| Phase | Nazwa | Główne wartości |
|---|---|---|
| Phase 1 | Quantity | `min_tx_count=12`, `min_unique_signers=8`, `min_buy_count=6`, `max_wait_time_ms=10000` |
| Phase 2 | Velocity | `max_avg_interval_ms=450`, `max_interval_cv=2.3`, `max_burst_ratio=0.72` |
| Phase 3 | Diversity | `max_hhi=0.155`, `max_volume_gini=0.70`, `max_top3_volume_pct=0.53`, `min_unique_signer_ratio=0.55` |
| Phase 4 | Volume | `min_buy_ratio=0.80`, `min_sol_buy_ratio=0.55`, `max_avg_tx_sol=0.45`, `max_volume_cv=1.20`, `max_consecutive_buys=3` |
| Phase 5 | Dev | `max_dev_buy_sol=2.0`, `max_dev_volume_ratio=0.23`, `reject_on_dev_sell=false` |
| Phase 6 | Curve | `max_price_change_ratio=1.50`, `min_market_cap_sol=41.0` w working tree, `min_bonding_progress_pct=40.0`, `max_bonding_progress_pct=99.0` |

`min_market_cap_sol=41.0` jest stanem roboczym, nie czystym `HEAD`.

### 3.2 V2.5 rollout config

W `[gatekeeper_v2.v25]`:

- `shadow_enabled = true`
- `live_execution_enabled = false`
- `require_promotion_adr = true`
- `emit_shadow_decisions = true`
- `emit_ablation_fields = true`

Znaczenie: V2.5 działa jako shadow/evidence plane. Nie promuje PDD/APS/DOW do
live hard veto bez jawnej decyzji/promocji.

### 3.3 DOW - Dynamic Observation Window

Aktualne okna:

- Early: `2000..5000 ms`, `early_min_confidence=0.85`,
  `early_min_tx=15`, `early_min_phases=6`,
  `early_min_momentum_score=0.40`, `early_max_sybil_score=1.0`,
  `early_max_entry_drift_pct=3.0`.
- Normal: `normal_window_ms=7000`, `normal_min_confidence=0.65`.
- Extended: `extended_window_ms=10000`,
  `extended_min_confidence=0.55`, `extended_require_pdd_clean=true`.
- Timer: `dow_tick_ms=250`.

DOW shadow checkpoints są odpalane przez TX path i timer path, ale nie mutują
terminalnego live verdictu. Są dowodem, co V2.5 zrobiłby wcześniej/później.

### 3.4 TAS - Trajectory Aware Scoring

Aktualnie:

- `tas.enabled = true`
- `tas_min_tx_per_segment = 3`
- `tas_min_total_duration_ms = 3000`
- `tas_hard_reject_threshold = 0.30`
- `tas_confidence_modulator_min = 0.75`
- `tas_confidence_modulator_max = 1.25`

TAS dzieli okno obserwacji na T0/T1/T2 i ocenia momentum, HHI trajectory,
volume consistency, interval trajectory i buy-ratio stability.

### 3.5 PDD - Pump & Dump Detector

Aktualnie:

- `pdd.enabled = true`
- `entry_drift_max_pct = 5.0`
- `entry_drift_soft_max_pct = 3.0`
- `spike_detection_enabled = true`
- `spike_ratio_threshold = 2.0`
- `spike_hard_veto = true`
- `ramping_detection_enabled = true`
- `ramping_min_consecutive_buys = 4`
- `ramping_hard_veto = true`
- `whale_top3_max_pct = 60.0`
- `reserve_min_sol = 30.0`
- `reserve_drop_max_pct = 0.15`
- `flash_crash_protection_enabled = true`
- `flash_crash_max_price_impact_pct = 15.0`
- `flash_crash_window_ms = 500`

PDD jest shadow-first. Live hard veto wymaga `v25.live_execution_enabled=true`
oraz odpowiedniej promocji progu. Przy aktualnym profilu V2.5 wykrycia PDD są
evidence/shadow, nie live kill-switch.

### 3.6 APS - Adaptive Prosperity

Aktualnie:

- `aps.enabled = true`
- `adaptive_enabled = false`
- `shadow_suggestions_enabled = true`
- `min_calibration_samples = 30`
- `regime_local_heuristic_enabled = true`
- `cross_pool_outcome_tracker_available = false`

APS generuje shadow/offline sugestie progów dla prosperity filter. Nie mutuje
aktywnych progów live. Overlay `configs/rollout/shadow-burnin.toml` jawnie
odsyła do brain configu jako SSOT APS.

## 4. Active vs legacy map

| Obszar | Status | Uwagi |
|---|---|---|
| `Seer` Yellowstone/gRPC -> IPC | aktywne | Produkcyjny ingest dla shadow-burnin, `source_mode="grpc"` |
| `GhostEvent::NewPoolDetected` | aktywne | Otwiera session i observation task |
| `GhostEvent::PoolTransaction` | aktywne | Główny strumień transakcji poola |
| `GhostEvent::FundingTransferObserved` | aktywne | Dane FSC/sybil/funding provenance |
| `GhostEvent::AccountUpdate` | aktywne warunkowo | Zależy od canonical account update relay |
| `OracleRuntime::pool_observation_task` | aktywne | Per-pool orchestration |
| `PoolObservationSession::materialize_features()` | aktywne SSOT | Główna granica dowodowa |
| `GatekeeperBuffer::evaluate_from_features()` | aktywne | Terminalny feature-driven evaluation path |
| `GatekeeperBuffer::try_shadow_evaluate()` | aktywne shadow | DOW checkpoint evidence; nie terminalny live verdict |
| `DecisionLogger` schema v19 | aktywne | JSONL, plane routing, reason code enforcement |
| `TriggerEntryMode::ShadowOnly` | aktywne dla burnin | Simulate transaction; no live send |
| `OracleRuntime::score_pool()` | legacy/deprecated | Nie jest aktywnym scorerem V2.5 |
| `GhostEvent::PoolScored` | legacy observation | Nie jest źródłem terminalnej decyzji |
| HyperPrediction/Chaos | legacy | Nie brać do architektury V2.5 |

## 5. Ingest: Seer, Yellowstone, parsery i IPC

### 5.1 Transport

Aktywny shadow-burnin używa Seer w profilu:

- `source_mode = "grpc"`
- `commitment = "processed"`
- `stream_mode = "single_global"`
- `tx_filter_strategy = "per_pool"`
- `funding_lane_mode = "full_chain"`
- `enable_pumpfun = true`
- `enable_pumpswap = true`

Seer obsługuje raw eventy typu:

- `GeyserEvent::Transaction`
- `GeyserEvent::AccountUpdate`
- `GeyserEvent::EntryAnchor`
- slot/backfill variants po stronie gRPC adaptera.

Własność Seer:

- połączenie do źródła danych,
- filtrowanie programów Pump/PumpSwap,
- parsowanie transakcji i account updates,
- normalizacja timestampów i slot metadata,
- deduplikacja/identity na poziomie źródła,
- emitowanie IPC eventów do launcher bridge.

Seer nie powinien podejmować decyzji inwestycyjnej. Seer produkuje dane.

### 5.2 Timestamp i provenance

Seer rozróżnia:

- chain event time, gdy dostępny z block time,
- ingress wall time, gdy chain time nie jest dostępny,
- arrival monotonic time,
- slot quality,
- event semantic envelope.

To jest ważne, bo Gatekeeper używa krótkich okien 2-10 sekund. Nie wolno
mieszać czasu łańcuchowego, czasu przyjęcia eventu i lokalnego wall-clock bez
jawnej jakości/provenance.

### 5.3 Parser curve/account data

Bonding curve parser obsługuje realne layouty:

- 56 bajtów bez Anchor discriminatora,
- 83/151+ bajtów z 8-bajtowym Anchor discriminatorem,
- zakres 49-82 bajty z heurystyką offsetu.

Parser waliduje sensowność virtual reserves. Dane curve są potem niesione do:

- `PoolTransaction.reserve_base`,
- `PoolTransaction.reserve_quote`,
- `PoolTransaction.price_quote`,
- `PoolTransaction.curve_data_known`,
- `PoolTransaction.curve_finality`,
- `AccountUpdateEvent` i `AccountStateCore`.

### 5.4 CandidatePool

`CandidatePool` reprezentuje wykryty nowy pool. Istotne pola:

- `pool_amm_id`
- `base_mint`
- `quote_mint`
- `bonding_curve`
- `creator`
- `slot`
- `event_ts_ms` / `event_time`
- `signature`
- initial liquidity / reserves / supply, jeśli dostępne.

Bridge mapuje go do `DetectedPool`, a następnie do
`GhostEvent::NewPoolDetected`.

### 5.5 TradeEvent -> PoolTransaction

`TradeEvent` z Seer jest kanonicznie mapowany do `PoolTransaction` w launcherze.
To jest granica:

```text
Seer TradeEvent
  -> trade_event_to_pool_transaction()
  -> GhostEvent::PoolTransaction
```

Zachowywane grupy pól:

- identity: `pool_amm_id`, `mint/token_mint`, `signature`, `event_ordinal`,
  `slot`,
- ordering/provenance: `timestamp_ms`, `event_time`, `arrival_ts_ms`,
  instruction provenance, CPI stack,
- actor: `signer`, `is_dev_buy`,
- trade semantics: `is_buy`, `volume_sol`, `sol_amount_lamports`,
  `token_amount_units`,
- curve: reserves, price quote, market cap, curve finality,
- execution fingerprints: CU price, CU limit, CU consumed, inner ix count,
  CPI depth, ATA count, Jito tip,
- owner token deltas and MPCF/toolchain fingerprint inputs.

`PoolTransaction` jest głównym eventem transakcyjnym konsumowanym przez
observation session.

### 5.6 Session gate po stronie Seer bridge

Seer bridge nie spamuje całego runtime wszystkimi trade'ami z chaina. Działa
session gate:

- `PoolDetected` rejestruje pool jako obserwowany.
- Trade przed rejestracją może być buforowany krótko.
- Po `PoolDetected` bridge replayuje gotowe trade'y dla tego poola.
- Trade bez forwardable identity jest odrzucany przed EventBus.
- AccountUpdate ma analogiczny bridge/buffer, gdy canonical relay jest włączony.

To chroni Gatekeepera przed ocenianiem pooli, których runtime faktycznie nie
otworzył.

### 5.7 Funding lane

`FundingTransferObserved` przenosi:

- source wallet,
- recipient wallet,
- lamports,
- slot/event ordinal/provenance,
- full-chain coverage flag,
- producer sequence number.

Te dane zasilają funding-source / FSC / sybil resistance, ale same nie są
terminalnym verdictem.

## 6. EventBus i runtime plane

`GhostEvent` jest busowym modelem zdarzeń. Aktywne dla Gatekeepera V2.5:

- `NewPoolDetected`
- `PoolTransaction`
- `FundingTransferObserved`
- `AccountUpdate`
- `GatekeeperCommitted`
- `ShadowBuySimulated`
- `PostBuySubmitted`

Legacy/observability:

- `PoolScored` jest klasyfikowany jako legacy observation plane.
- Nie wolno traktować go jako źródła aktualnej decyzji.

EventBus używa broadcast semantics. Konsekwencje:

- event może mieć wielu odbiorców,
- opóźniony odbiorca może lagować,
- runtime musi jawnie obsługiwać closed/lagged branches,
- decyzja Gatekeepera nie może zależeć od ukrytego side-effectu odbiorcy, który
  nie jest częścią materializacji.

## 7. OracleRuntime: routing i per-pool lifecycle

`OracleRuntime` jest aktywnym koordynatorem:

- rejestruje nowe poole,
- tworzy per-pool observation task,
- utrzymuje `SessionManager`,
- utrzymuje `AccountStateCore`,
- remapuje identity pool/base_mint,
- buforuje orphan tx, jeśli TX przyszedł przed rejestracją,
- obsługuje `AccountUpdate` worker,
- egzekwuje deadline/DOW timers,
- loguje verdicty i handoff do triggera.

### 7.1 NewPoolDetected

Po `GhostEvent::NewPoolDetected` runtime:

1. normalizuje identity pool/base_mint,
2. rejestruje identity w registry,
3. tworzy/open session przez `SessionManager`,
4. startuje `pool_observation_task`,
5. replayuje orphan tx, jeśli były,
6. zakłada deadline z configu Gatekeepera.

Jeden pool powinien mieć jedną aktywną observation session.

### 7.2 PoolTransaction

Po `GhostEvent::PoolTransaction` runtime:

1. rozwiązuje pool identity,
2. odrzuca/re-mapuje niespójności base_mint/pool_amm_id,
3. jeśli pool jest już committed/approved, może kierować TX do post-commit path,
4. jeśli pool ma aktywny observation task, wysyła TX do tego taska,
5. jeśli session jeszcze nie istnieje, buforuje orphan tx.

Nie ma tutaj policy decision. To routing danych.

### 7.3 AccountUpdate

`AccountUpdate` może zasilać `AccountStateCore`, gdy canonical relay jest
aktywny. Aktualizacja ma:

- base mint,
- bonding curve,
- reserves,
- slot,
- optional write_version,
- receive sequence,
- curve finality.

`AccountStateCore` akceptuje tylko monotonicznie nowsze update'y:

1. większy slot,
2. albo ten sam slot i większy `write_version`,
3. albo ten sam slot/write_version i większy receive sequence.

Stare/duplikowane update'y są odrzucane jako stale evidence.

### 7.4 Deadline i DOW timer

Per-pool task używa `tokio::select!` dla:

- TX z kanału,
- DOW timer tick,
- hard deadline,
- channel close/shutdown.

DOW timer ma `MissedTickBehavior::Skip`, aby nie nadrabiać zaległych ticków
hurtem. Terminalny deadline pozostaje single final evaluation. DOW checkpointy
są shadow evidence.

## 8. PoolObservationSession

`PoolObservationSession` jest lokalnym stanem obserwacji poola. Posiada:

- `GatekeeperBuffer`,
- `TxIntelligenceEngine`,
- `AccountStateCore` reference,
- `CheckpointEngine`,
- `ObservationFeatureBuilder`,
- bounded tx buffer,
- CPV index,
- FSC index,
- diagnostics/session metadata.

### 8.1 TX ingest w session

`ingest_transaction()` robi kilka rzeczy, ale nadal nie wydaje terminalnej
decyzji:

1. podaje TX do `TxIntelligenceEngine`,
2. podaje TX do `GatekeeperBuffer::ingest_transaction_tracking_only`,
3. aktualizuje CPV/FSC,
4. dodaje TX do bounded ring buffer,
5. aktualizuje diagnostics.

`tracking_only` jest ważne: active terminal evaluation ma iść z
`MaterializedFeatureSet`, a nie z natychmiastowej decyzji przy każdej transakcji.

### 8.2 Account state w session

Session może:

- przyjąć `AccountStateUpdate`,
- zsynchronizować się z `AccountStateCore`,
- użyć fallbacków, jeśli canonical account update nie jest jeszcze dostępny.

Fallback nie może udawać canonical evidence. W materializacji musi być widoczne,
czy dane curve/account są bootstrap/provisional/canonical.

### 8.3 Checkpoints

`CheckpointEngine` i `ObservationFeatureBuilder` produkują checkpoint-derived
features, w tym trajectory assessment, jeśli dane są wystarczające. To jest
Path A dla TAS.

## 9. Właściciele danych i feature'ów

| Obszar dowodów | Główny właściciel | Trafia do |
|---|---|---|
| Pool identity | Seer bridge + `PoolIdentityRegistry` | session/runtime/logs |
| Tx count/buy/sell/signers/volume/timing | `TxIntelligenceEngine` | `tx_intel_features` |
| Dev behavior | `TxIntelligenceEngine` | `tx_intel_features` |
| Signer diversity/HHI/Gini/top3 | `TxIntelligenceEngine` | phase 3 |
| Curve reserves/price/mcap/progress | `AccountStateCore` + parser/fallback | `account_features`, `curve_readiness` |
| Checkpoint trajectory | `CheckpointEngine` | `checkpoint_features.trajectory_assessment` |
| Raw T0/T1/T2 segment sequence | `GatekeeperBuffer` | `tx_segment_sequence` |
| Early fingerprint metrics | `TxIntelligenceEngine` / Seer fingerprint agg | `alpha_fingerprint` |
| Sybil resistance | tx-intelligence sybil metrics, CPV/FSC | `sybil_resistance` |
| Funding provenance | Funding lane + FSC index | sybil/funding diagnostics |
| Session metadata | `PoolObservationSession` | `session_metadata` |
| Risk flags | `TxIntelligenceEngine` | `risk_flags` |

## 10. MaterializedFeatureSet: główna granica SSOT

`MaterializedFeatureSet` zawiera:

- `account_features`,
- `tx_intel_features`,
- `checkpoint_features`,
- `risk_flags`,
- `session_metadata`,
- `curve_readiness`,
- `sybil_resistance`,
- `alpha_fingerprint`,
- `tx_segment_sequence`.

To jest snapshot, który Gatekeeper ma oceniać. Po tej granicy policy nie powinna
iść do live mutable state po własne, alternatywne feature'y.

### 10.1 Materializacja krok po kroku

`PoolObservationSession::materialize_features()`:

1. pobiera aktualne account features z `AccountStateCore` albo jawnego fallbacku,
2. pobiera `TxIntelFeatures` z `TxIntelligenceEngine`,
3. pobiera checkpoint-derived features z `ObservationFeatureBuilder`,
4. aktualizuje trajectory assessment,
5. dołącza `tx_segment_sequence` z `GatekeeperBuffer`,
6. dołącza curve readiness/finality,
7. dołącza alpha fingerprint metrics,
8. dołącza sybil resistance/CPV/FSC diagnostics,
9. dołącza risk flags i session metadata.

### 10.2 Path A i Path B

TAS i część PDD mają dwa źródła dowodowe:

- Path A: `checkpoint_features.trajectory_assessment`.
- Path B: `tx_segment_sequence` z T0/T1/T2.

Path B nie jest legacy. Jest aktywnym fallbackiem i explicit SSOT extension dla
feature-driven policy. Jeżeli Path A nie istnieje, policy może zrekonstruować
TAS z Path B, o ile segmenty spełniają warunki minimalne.

`tx_segment_sequence` jest materializowany niezależnie od `tas.enabled`, bo PDD
też potrzebuje sekwencji. To zapobiega sytuacji, w której wyłączenie TAS
przypadkiem pozbawia PDD danych o spike/ramping.

## 11. AccountStateCore

`AccountStateCore` jest canonical reducerem stanu kont poola:

- `CanonicalPoolState`,
- `AccountStateUpdate`,
- `AccountStateFeatures`,
- `StatePhase`.

Liczy:

- virtual SOL/token reserves,
- real SOL/token reserves,
- price in SOL,
- market cap in SOL,
- bonding progress,
- price change since T0,
- reserve velocity,
- curve finality,
- update count.

Ważne zasady:

- update'y są monotoniczne po `(slot, write_version, recv_seq)`,
- `write_version=None` jest normalizowany jako wysoka wartość dla same-slot
  ordering,
- old/stale update nie może nadpisać canonical state,
- bootstrap state może zostać promowany po pierwszym real account update,
- state phase mówi, czy dane są bootstrap-like, canonical czy migrated.

## 12. TxIntelligenceEngine

`TxIntelligenceEngine` agreguje transakcje w oknie obserwacji. Liczy:

- total tx,
- buy count,
- sell count,
- unique signers,
- buy ratio,
- SOL buy ratio,
- avg/min/max TX SOL,
- volume CV,
- HHI,
- volume Gini,
- top3 volume pct,
- unique signer ratio,
- same-ms tx ratio,
- bundle suspicion ratio,
- max tx per signer,
- max consecutive buys,
- dust tx,
- failed tx,
- dev buy/sell behavior,
- dev volume ratio,
- timing entropy,
- avg interval,
- burst ratio.

TX dedup używa `TxKey` z:

- event timestamp,
- slot,
- event ordinal,
- signature,
- fallback counter.

To pozwala odróżnić kilka semantic trade events w jednej transaction signature.

### 12.1 Risk flags

Risk flags mogą oznaczać m.in.:

- developer sold,
- extreme bot timing,
- failed tx ratio,
- dust/excessive burst patterns,
- inne twarde/miękkie sygnały z tx-intelligence.

Hard risk flag nie powinien być zasłonięty soft score.

### 12.2 Early fingerprint

Early fingerprinting korzysta z realnych pól tx-meta:

- block0 sniped supply,
- flip ratio,
- CU price p90,
- priority fee surge slope,
- buyer pre-balance CV,
- avg inner ix count,
- avg CPI depth,
- sell/buy ratio,
- compute-unit cluster dominance,
- static fee profile ratio,
- fixed-size buy ratio,
- flipper presence,
- Jito tip intensity,
- early-slot volume dominance,
- early top3 buy concentration,
- whale reversal,
- dev paperhand latency.

Jeżeli dane są niedostępne, fingerprint musi oznaczać degraded reason, a nie
udawać zero-risk.

### 12.3 Sybil metrics

Sybil resistance opiera się m.in. na:

- CPV - cross-pool velocity,
- FSC - funding-source correlation,
- FTDI - fee topology diversity,
- DBIA - dev-buyer infrastructure affinity,
- SFD - spend fraction divergence,
- DES - demand elasticity score.

Te pola mają degraded reasons, np. brak raw fee topology, brak dev buy, brak
slot ordering, brak postbalance. Dla eksperta biznesowego degraded oznacza
niepewność, nie pozytywny sygnał.

## 13. GatekeeperBuffer

`GatekeeperBuffer` w V2.5 pełni kilka ról:

- utrzymuje buffered txs,
- deduplikuje TX keys,
- śledzi phase counters,
- śledzi price history,
- śledzi curve readiness/latch,
- materializuje T0/T1/T2 sequence,
- odpala DOW shadow checkpoints,
- przechowuje `v25_shadow_decisions` do późniejszego logowania.

Nie jest już jedynym źródłem terminalnej decyzji w aktywnej ścieżce. Terminalny
path bierze snapshot z session i woła `evaluate_from_features()`.

### 13.1 DOW checkpoint state

DOW checkpointy mają stage:

- Early,
- Normal,
- Extended.

Źródło checkpointu:

- TX path,
- Timer path,
- Deadline fallback.

Każdy stage jest one-shot. Runtime pilnuje flags, żeby TX i timer nie odpaliły
podwójnego shadow decision dla tego samego stage.

## 14. Gatekeeper policy V2 core

Aktywny terminalny call chain:

```text
OracleRuntime::evaluate_feature_driven_terminal_verdict()
  -> PoolObservationSession::materialize_features()
  -> GatekeeperBuffer::prepare_feature_evaluation()
  -> GatekeeperBuffer::evaluate_from_features(features, config)
  -> build_assessment_from_features()
  -> evaluate_policy_from_assessment()
  -> evaluate_curve_gate()
  -> GatekeeperVerdict::{Buy, Reject, Timeout, PendingCurve}
```

### 14.1 Assessment construction

`build_assessment_from_features()` mapuje `MaterializedFeatureSet` na:

- phase 1 quantity,
- phase 2 velocity,
- phase 3 diversity,
- phase 4 volume,
- phase 5 dev behavior,
- phase 6 curve,
- risk flags,
- sybil diagnostics,
- alpha diagnostics,
- prosperity diagnostics,
- PDD diagnostics,
- APS diagnostics,
- TAS assessment,
- observation stage,
- V2.5 confidence availability.

### 14.2 Policy order

Ewaluacja policy idzie w porządku:

1. Hard filters z feature snapshotu i risk flags.
2. PDD live hard veto tylko gdy V2.5 live execution i próg jest promowany.
3. Core phase fail.
4. Sybil combo / sybil interference / sybil soft excess.
5. Legacy soft excess.
6. Alpha gate.
7. Prosperity filter.
8. TAS modulation / low trajectory handling.
9. BUY.

To jest ważne: soft pozytywny wynik nie może przykryć twardego sygnału ryzyka.

### 14.3 Sześć faz

Phase 1 - Quantity:

- minimalna liczba transakcji,
- minimalna liczba unikalnych signerów,
- minimalna liczba buy,
- deadline okna.

Phase 2 - Velocity:

- średni interwał,
- interval CV,
- burst ratio,
- timing entropy.

Phase 3 - Diversity:

- HHI signer/volume concentration,
- volume Gini,
- top3 volume,
- unique signer ratio,
- max tx per signer.

Phase 4 - Volume:

- buy ratio,
- SOL buy ratio,
- avg tx SOL,
- volume CV,
- max consecutive buys,
- fingerprint/alpha choke inputs.

Phase 5 - Developer:

- dev buy SOL,
- dev volume ratio,
- dev tx ratio,
- dev sold,
- dev first buyer / initial tokens.

Phase 6 - Curve:

- price change ratio,
- market cap SOL,
- bonding progress min/max,
- curve data known,
- curve finality.

## 15. V2.5 modules

### 15.1 DOW - Dynamic Observation Window

DOW odpowiada na pytanie: czy Gatekeeper mógłby zdecydować wcześniej albo
później niż klasyczne 10 sekund, i co to mówi o jakości scoringu?

Mechanika:

- Early checkpoint sprawdza szybki, mocny sygnał.
- Normal checkpoint reprezentuje standardowe okno.
- Extended checkpoint reprezentuje cierpliwszą ocenę przy słabszej pewności.
- Shadow decision jest logowany jako evidence.
- Terminalny live verdict nie jest mutowany przez shadow checkpoint.

`GatekeeperBuffer::new()` fail-fast sprawdza, że `extended_window_ms` nie
przekracza `max_wait_time_ms`. Przy aktualnym configu oba wynoszą 10000 ms.

### 15.2 TAS - Trajectory Aware Scoring

TAS odpowiada na pytanie: czy aktywność poola wygląda jak zdrowa trajektoria,
czy tylko jak chwilowy peak?

Dzieli okno na trzy segmenty:

- T0 - początek,
- T1 - środek,
- T2 - końcówka.

Liczy:

- momentum score: T2/T0 tx count,
- HHI trajectory: spadek koncentracji jest dobry,
- volume consistency: zbyt niestabilny wolumen obniża score,
- interval trajectory: skracające się interwały mogą być pozytywnym momentum,
- buy ratio stability.

Wynik `overall_tas_score` mapuje się na confidence modulator
`0.75..1.25`. Bardzo niski score może tworzyć low trajectory reject/demotion.

### 15.3 PDD - Pump & Dump Detector

PDD odpowiada na pytanie: czy wejście już jest spóźnione albo czy widzimy
schemat pompy/dumpu?

Path A z buffer/price history:

- entry drift z hierarchią anchorów,
- spike,
- ramping,
- whale concentration,
- reserve health,
- flash crash.

Entry drift anchor hierarchy:

1. init-pool authoritative: curve data known i real reserve,
2. AccountStateCore reserve,
3. parser-authoritative curve data,
4. fallback price history.

Path B z `tx_segment_sequence`:

- spike z T2 volume rate vs wcześniejsze segmenty,
- ramping z same-size streak w T1/T2,
- flash crash jest jawnie unavailable, bo Path B nie ma price impact data.

To nie jest brak ryzyka. To brak dowodu. Dokumenty/raporty powinny traktować
unavailable jako osobny stan.

### 15.4 APS - Adaptive Prosperity

APS odpowiada na pytanie: czy progi prosperity powinny zależeć od reżimu rynku?

Aktualnie działa jako shadow/offline suggestions:

- wykrywa `Normal`, `HighVolatility`, `LowVolatility`,
- ma calibration guard `min_calibration_samples`,
- w aktualnym profilu `adaptive_enabled=false`,
- nie mutuje live thresholds.

APS może powiedzieć, czy prosperity filter przeszedłby przy progach
kontrfaktycznych. To jest materiał do kalibracji, nie bezpośrednia zmiana BUY.

### 15.5 V2.5 confidence

V2.5 confidence jest multiplicative:

```text
base_quality
  * alpha_quality
  * pdd_modulator
  * tas_modulator
  * sybil_modulator
```

PDD hard fail lub TAS hard reject może wyzerować confidence. Brak danych ma być
raportowany przez availability/degraded fields, a nie ukrywany jako czysty
wynik.

## 16. Curve gate

Curve gate jest osobnym krokiem po policy decision. Nawet jeżeli policy mówi
BUY, curve gate może:

- przepuścić, gdy curve jest ready/fresh/committed,
- zwrócić `PendingCurve`, gdy dane są nieznane i jest jeszcze czas,
- odrzucić, gdy curve timeout/stale policy wymaga reject,
- dopuścić stale tylko przy jawnej polityce fallback.

`PendingCurve` nie jest terminalnym verdictem. Runtime rollbackuje feature
evaluation i czeka dalej, o ile deadline/policy na to pozwala.

Curve policy jest synchronizowana z `[shadow_ledger]` jako SSOT w launcherze.
Jeżeli Gatekeeper config i shadow ledger config różnią się w `curve_wait_ms`,
`curve_require_for_buy` albo `stale_fallback`, startup nadpisuje policy
Gatekeepera wartościami z shadow ledger config.

## 17. Verdicts i reason codes

Terminalne i pomocnicze typy:

- `Wait` - wewnętrzne, nieterminalne.
- `PendingCurve` - nieterminalne oczekiwanie na curve evidence.
- `Buy` - Gatekeeper approve.
- `Reject` - terminalne odrzucenie.
- `Timeout` - terminalny brak wystarczających danych/faz w deadline.
- `ApprovedTx` - post-approval event type, nie główny feature evaluation verdict.

`GatekeeperVerdictType` obejmuje m.in.:

- BUY / EARLY_BUY / EXTENDED_BUY,
- hard fail classes,
- core fail,
- sybil rejects,
- alpha/prosperity rejects,
- PDD rejects,
- TAS/low trajectory rejects,
- IWIM rejects,
- timeout subtypes.

`GatekeeperReasonCode` v2 zapisuje stabilne `SCREAMING_SNAKE_CASE`, np.:

- `BUY_NORMAL`, `BUY_EARLY`, `BUY_EXTENDED`,
- `HARD_FAIL_DEV_SOLD`, `HARD_FAIL_MARKET_CAP`,
- `REJECT_PDD_ENTRY_DRIFT`, `REJECT_PDD_SPIKE`,
- `REJECT_LOW_ALPHA`, `REJECT_LOW_PROSPERITY`,
- `REJECT_IWIM_VETO`, `REJECT_IWIM_LOW_CONF`,
- `REJECT_LOW_TRAJECTORY`,
- `TIMEOUT_PHASE1_NO_DATA`,
- `TIMEOUT_PHASE1_INSUFFICIENT`,
- `TIMEOUT_DEADLINE_LOW_PHASES`,
- invariant codes.

W schema v19 brak `reason_code` dla plane row jest traktowany fail-closed:
logger dropuje taki row zamiast tworzyć słabo audytowalny zapis.

## 18. IWIM: post-Gatekeeper BUY veto

IWIM działa po Gatekeeper BUY, przed trigger execution handoff.

Znaczenie:

- Gatekeeper może wydać BUY.
- IWIM może zamienić BUY na typed REJECT.
- Reason code musi wtedy przejść na `RejectIwim*`.
- Dopiero po przejściu IWIM runtime idzie do buy execution path.

IWIM nie powinien być mylony z fazami Gatekeepera. To jest post-GK safety gate.

## 19. Shadow-burnin: co to jest

Shadow-burnin to tryb, w którym Ghost:

1. używa realnego ingestu i realnej polityki Gatekeepera,
2. dopuszcza Gatekeeper BUY do trigger path,
3. buduje realnie uformowaną transakcję buy,
4. odpala RPC `simulate_transaction_with_config`,
5. zapisuje wynik symulacji, lifecycle i post-buy shadow evidence,
6. nie wysyła live transaction.

Aktualny profil:

- `trigger.entry_mode = "shadow_only"`
- `execution.execution_mode = "shadow"`
- `trigger.shadow_run.enabled = true`
- `trigger.shadow_run.payer_strategy = "ephemeral"`
- `trigger.shadow_run.sig_verify = false`
- `trigger.shadow_run.replace_recent_blockhash = true`
- `trigger.shadow_run.timeout_ms = 1600`
- `trigger.shadow_run.max_retries = 1`
- `trigger.shadow_run.max_concurrent = 8`
- shadow entry log: `data/shadow-burnin/shadow_entries.jsonl`
- shadow lifecycle log: `data/shadow-burnin/shadow_lifecycle.jsonl`

## 20. Shadow-burnin flow

```text
Gatekeeper BUY
  -> IWIM pass
  -> execute_gatekeeper_buy_path()
  -> hydrate metadata and shadow readiness
  -> prepare buy request in TriggerComponent
  -> shadow-only simulate_buy()
  -> RPC simulate_transaction_with_config()
  -> TriggerBuyOutcome::ShadowSimulated
  -> GhostEvent::ShadowBuySimulated
  -> append shadow buy record
  -> append shadow dispatch lifecycle record
  -> optional shadow-backed PostBuySubmitted
  -> DecisionLogger/shadow reports
```

### 20.1 Co shadow oddaje dobrze

Shadow-burnin dobrze oddaje:

- realne wejście danych Seer/Yellowstone,
- realny timing observation window,
- realne Gatekeeper policy i config,
- realny IWIM handoff,
- realną ścieżkę przygotowania requestu w triggerze,
- realny RPC simulation endpoint,
- realny compute/simulation error surface,
- przybliżony token delta z symulowanego ATA, jeśli RPC zwróci account state,
- latency od decyzji do symulacji,
- units consumed,
- logs excerpt/digest,
- retry/timeout behavior,
- lifecycle terminalność shadow dispatch.

### 20.2 Czego shadow nie dowodzi

Shadow-burnin nie dowodzi:

- że transakcja trafiłaby do bloku,
- że live sender miałby aktualny blockhash w tym samym momencie,
- że Jito/tip/priority fee wygrałby contention,
- że account contention i competing buys wyglądałyby identycznie,
- że live signature istnieje,
- że post-buy PnL jest realnym PnL,
- że submit oznacza confirmation.

`live_signature` w shadow-only jest `None`. `closed` w lifecycle oznacza
terminalny wynik symulacji/evidence, nie on-chain inclusion.

### 20.3 Shadow lifecycle identity

Shadow lifecycle używa:

- `join_key = pool_id:base_mint:first_seen_ts_ms`,
- `idempotency_key = blake3(pool_id:join_key:rollout_profile)`,
- `dispatch_id = shadow-dispatch:{idempotency_key}`.

`ShadowDispatchStatus`:

- `submitted` - dispatch/simulation task przyjęty,
- `closed` - symulacja zakończona bez `err`,
- `failed` - symulacja zakończona z błędem lub preparation failure,
- `abandoned` - task timeout/join error/utracony terminal.

Lifecycle row niesie:

- record type,
- dispatch id,
- idempotency key,
- dispatch status,
- classification,
- simulation outcome,
- candidate id,
- pool id,
- mint id,
- join key,
- rollout profile,
- entry mode,
- decision timestamp,
- terminal timestamp,
- error class/code/detail.

### 20.4 no_dispatch semantics

Reject/timeout bez BUY nie powinien mieć shadow dispatch lifecycle. To nie jest
błąd.

Runtime klasyfikuje:

- reject jako `no_dispatch_rejected`,
- timeout jako `no_dispatch_eligible` lub podobny no-dispatch outcome,
- report jako `no_dispatch_no_economics_required`, jeśli nie było dispatchu.

Economics/lifecycle proof jest wymagany dla rzeczywistych dispatch candidates,
nie dla pooli, które Gatekeeper odrzucił.

### 20.5 Shadow post-buy

Po `ShadowBuySimulated` runtime może uruchomić shadow-backed post-buy monitor.
To jest syntetyczne lifecycle/economics proof:

- exit filled,
- exit blocked,
- position closed,
- synthetic PnL fields.

Nadal jest to lane `shadow`. `PostBuySubmitted` w shadow lane jest handoffem do
monitoringu, nie dowodem live submitu.

## 21. Decision logging i replay

### 21.1 Schema

Aktualny DecisionLogger używa:

- `GATEKEEPER_BUY_LOG_SCHEMA_VERSION = 19`
- `GATEKEEPER_VERSION = "v2.5"`
- legacy version marker dla starszych plane rows.

Pliki:

- `gatekeeper_v2_decisions.jsonl` - wszystkie terminalne decyzje,
- `gatekeeper_v2_buys.jsonl` - BUY-related rows,
- shadow entry/lifecycle JSONL - trigger shadow evidence.

### 21.2 Plane routing

Logger rozdziela output po:

```text
{rollout_profile}/{gatekeeper_version}/{decision_plane}/{config_hash}
```

Kluczowe plane'y:

- `legacy_live`,
- `v25_shadow`.

Nie wolno agregować wyników bez wymiaru `decision_plane`. Dla eksperta
kalibracyjnego mieszanie live legacy i v25 shadow zniszczy interpretowalność.

### 21.3 GatekeeperBuyLog

Log niesie:

- identity: pool id, mint id, candidate id,
- rollout profile,
- config hash,
- gatekeeper version,
- decision plane,
- verdict/outcome,
- `verdict_type`,
- `reason_code`,
- `reason_code_version`,
- `terminal_reason_code`,
- V2 phase fields,
- V2.5 shadow decision fields,
- DOW stage,
- PDD diagnostics,
- TAS diagnostics,
- APS diagnostics,
- confidence and availability fields,
- alpha/prosperity/sybil diagnostics,
- IWIM fields,
- shadow readiness/execution outcome,
- no-dispatch classifications,
- timestamps/window/join key.

### 21.4 Raporty

`scripts/shadow_run_report.py`:

- zbiera decision/buy rows,
- wybiera właściwy plane,
- liczy expected dispatch candidates,
- liczy actual dispatch rows,
- liczy terminal lifecycle rows,
- rozróżnia no-dispatch od lifecycle failure,
- nie wymaga economics, gdy nie było dispatchu.

`scripts/gatekeeper_v25_repair_validation.py`:

- sprawdza reason-code completeness,
- sprawdza timeout taxonomy,
- sprawdza rollout/plane scope,
- odpala shadow report,
- klasyfikuje P5 no-dispatch vs dispatch lifecycle.

Replay equivalence nie opiera się na gołych event JSONL jako jedynym źródle
prawdy. Potrzebny jest decision log plus config/profile/plane/schema/reason
codes oraz shadow lifecycle dla dispatch candidates.

## 22. Co ekspert biznesowy powinien czytać jako sygnał

### 22.1 BUY nie znaczy "zarobimy"

BUY znaczy: według aktualnych progów i dostępnego evidence pool przeszedł
Gatekeepera oraz IWIM. W shadow-only nie ma live inclusion. BUY jest hipotezą
handlową do walidacji przez shadow lifecycle i post-buy outcome.

### 22.2 REJECT powinien być konkretny

Dobry reject ma:

- `verdict_type`,
- `reason_code`,
- fazy,
- konkretne diagnostics,
- availability/degraded reasons, jeśli brakuje danych.

Generic reject jest słabym materiałem do kalibracji.

### 22.3 Timeout jest informacją

Timeout może znaczyć:

- brak danych,
- za mało tx/signers/buys,
- zbyt mało faz przed deadline,
- curve not ready,
- stale/unknown evidence.

Timeout nie powinien być traktowany jako neutralny discard. To jest sygnał o
coverage, timing albo jakości ingestu.

### 22.4 Shadow no-dispatch jest poprawne dla reject/timeout

Jeśli Gatekeeper odrzucił pool, brak dispatchu jest oczekiwany. Shadow
lifecycle ma rozliczać tylko kandydatów, którzy dotarli do dispatch/simulation.

### 22.5 Degraded fields są krytyczne

Sygnały sybil/fingerprint/PDD/TAS mogą być unavailable. Brak danych nie jest
automatycznie pozytywny. W kalibracji trzeba rozdzielić:

- signal clean,
- signal bad,
- signal unavailable,
- signal degraded with reason.

## 23. Najważniejsze pliki

Ingest:

- `off-chain/components/seer/src/lib.rs`
- `off-chain/components/seer/src/config.rs`
- `off-chain/components/seer/src/grpc_connection.rs`
- `off-chain/components/seer/src/types.rs`
- `off-chain/components/seer/src/curve_parser.rs`
- `off-chain/components/seer/src/early_fingerprint.rs`
- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/events.rs`

Runtime/session:

- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/session/manager.rs`
- `ghost-launcher/src/session/observation.rs`
- `ghost-launcher/src/components/gatekeeper_dow_timer.rs`

SSOT/state:

- `ghost-core/src/checkpoint/types.rs`
- `ghost-core/src/checkpoint/feature_builder.rs`
- `ghost-core/src/account_state_core/types.rs`
- `ghost-core/src/account_state_core/reducer.rs`
- `ghost-core/src/account_state_core/monotonic_guard.rs`
- `ghost-core/src/tx_intelligence/types.rs`

Gatekeeper:

- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/src/components/gatekeeper_pdd.rs`
- `ghost-launcher/src/components/gatekeeper_pdd_sequence.rs`
- `ghost-launcher/src/components/gatekeeper_trajectory.rs`
- `ghost-launcher/src/components/gatekeeper_adaptive_prosperity.rs`

Shadow execution:

- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-launcher/src/components/trigger/shadow_run.rs`
- `ghost-brain/src/guardian/post_buy/engine.rs`

Config/logging/report:

- `ghost-brain/ghost_brain_config.toml`
- `configs/rollout/shadow-burnin.toml`
- `ghost-brain/src/config/gatekeeper_v25_config.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- `ghost-brain/src/oracle/reason_code.rs`
- `scripts/shadow_run_report.py`
- `scripts/gatekeeper_v25_repair_validation.py`

## 24. Testy regresyjne, które chronią kontrakty

Najważniejsze anchors:

- `ghost-launcher/tests/gatekeeper_v25_regression.rs`
  - shadow nie mutuje live verdict,
  - DOW odpala stage bez TX pressure,
  - `MaterializedFeatureSet` niesie `tx_segment_sequence`,
  - Path A/Path B TAS parity,
  - APS działa w Path B,
  - IWIM BUY -> REJECT mutuje reason code.
- `ghost-launcher/tests/gatekeeper_policy_tests.rs`
  - feature API matchuje assessment snapshot,
  - session features drive policy BUY.
- `ghost-launcher/tests/gatekeeper_pdd_tests.rs`
  - PDD entry drift hard reject.
- `ghost-launcher/tests/gatekeeper_tas_tests.rs`
  - TAS momentum/trajectory behavior.
- `ghost-launcher/src/oracle_runtime.rs` tests
  - shadow-only emituje simulated buy,
  - shadow-only nie emituje live `TransactionSent`.
- `ghost-brain/src/config/gatekeeper_v25_config.rs` tests
  - serde/default compatibility.
- `scripts/test_shadow_run_report.py`
  - no-dispatch nie powoduje fałszywego failure.

## 25. Aktualne ryzyka i caveaty

1. `ghost-brain/ghost_brain_config.toml` był dirty w czasie audytu.
   Dokument opisuje stan roboczy, ale przy czystym `HEAD` próg market cap może
   być inny.
2. Komentarz w `configs/rollout/shadow-burnin.toml` wspomina stare
   `max_wait_time_ms=8001`; aktualny brain config ma `10000`.
3. Shadow-burnin jest bardzo dobrym dowodem simulation/execution-shape, ale nie
   dowodzi live inclusion.
4. APS adaptive thresholds są obecnie shadow/offline. Nie wolno zakładać, że
   realnie zmieniają progi live.
5. PDD Path B nie potrafi uczciwie wykryć flash crash bez price impact data.
   To jest unavailable, nie pass.
6. Plane routing jest obowiązkowy w analizie. `legacy_live` i `v25_shadow` nie
   mogą być mieszane.
7. Event JSONL sam w sobie nie jest wystarczającym dowodem audytowym decyzji.
   Potrzebny jest config, reason-code taxonomy, decision plane i lifecycle.

## 26. Najkrótszy przewodnik dla Mr. Guru

Jeżeli celem jest poprawa realnej rentowności oceny tokenów, zacząć od tych
warstw:

1. Jakość danych wejściowych:
   - coverage Seer,
   - timestamp quality,
   - AccountUpdate availability,
   - degraded reasons w fingerprint/sybil.
2. Jakość negative filters:
   - PDD entry drift,
   - spike/ramping,
   - whale concentration,
   - dev behavior,
   - sybil/infra similarity.
3. Jakość positive selection:
   - TAS trajectory,
   - alpha fingerprint,
   - prosperity filter,
   - curve readiness/finality.
4. Jakość temporalna:
   - czy Early/Normal/Extended DOW daje lepszy tradeoff,
   - czy 10s deadline jest optymalny dla observed opportunity decay.
5. Jakość execution realism:
   - shadow simulation success vs failure,
   - latency decision-to-sim,
   - units consumed,
   - post-buy synthetic lifecycle,
   - no-dispatch separation.
6. Jakość audytu:
   - typed reason codes,
   - plane routing,
   - config hash,
   - lifecycle terminalność,
   - replay equivalence.

Najważniejsza zasada interpretacyjna:

```text
Nie optymalizować wyłącznie liczby BUY.
Optymalizować selektywność BUY przy pełnym rozumieniu:
  - dlaczego pool przeszedł,
  - jakie dane były niedostępne,
  - czy shadow execution byłby wykonalny,
  - czy post-buy lifecycle wyglądałby ekonomicznie sensownie.
```

## 27. Finalny status architektoniczny

Aktualny Gatekeeper V2.5 jest feature-driven i shadow-first:

- aktywny path decyzyjny jest oparty o `MaterializedFeatureSet`,
- V2.5 rozszerza V2 o DOW/TAS/PDD/APS/reason-code evidence,
- shadow-burnin symuluje execution shape bez live send,
- DecisionLogger v19 wymusza typed reason code i plane separation,
- legacy HyperPrediction/Chaos/`score_pool()` nie są częścią obecnego runtime.

To oznacza, że dalsze prace nad logiką biznesową powinny zaczynać się od
kalibracji feature'ów i progów w tym właśnie pipeline, a nie od reaktywacji
starych scoring engines.

## 28. Checklist pokrycia wymagań z promptu

Ta sekcja jest jawny indeksem, gdzie w dokumencie znajduje się minimalny zakres
oczekiwany dla eksperta przeglądającego Gatekeepera V2.5.

| Wymagany obszar | Gdzie jest opisany | Status |
|---|---|---|
| Aktualny flow decyzji BUY/REJECT/TIMEOUT | Sekcje 1, 14, 16, 17, 18, 21, 22 | Pokryte |
| Lista komponentów i ich odpowiedzialności | Sekcje 4, 5, 6, 7, 8, 9, 23 | Pokryte |
| `MaterializedFeatureSet` i feature ownership | Sekcje 2, 9, 10 | Pokryte |
| Rola `TxIntelligence`, `AccountStateCore`, Checkpoints, `GatekeeperBuffer` | Sekcje 8, 9, 11, 12, 13 | Pokryte |
| Gatekeeper V2/V2.5 i aktywne gate'y | Sekcje 3, 14, 15, 16, 18 | Pokryte |
| Obecne progi/configi i ich znaczenie | Sekcja 3 oraz caveaty w 25 | Pokryte |
| Active/shadow-only/diagnostic-only/legacy | Sekcje 0, 2, 4, 15, 19, 20, 27 | Pokryte |
| DecisionLogger / JSONL format | Sekcje 21.1-21.4 | Pokryte |
| Shadow-burnin | Sekcje 19, 20, 21, 22, 25 | Pokryte |
| Metryki i sygnały już zbierane | Sekcje 12.2, 12.3, 15.5, 20.1, 21.3, 26 oraz lista niżej | Pokryte |
| Znane problemy, false BUY/false REJECT, timeouty, degraded states | Sekcje 22, 25, 26 oraz lista niżej | Pokryte |
| Ograniczenia latency i observation window | Sekcje 3.3, 7.4, 15.1, 20.1, 20.2, 26 oraz lista niżej | Pokryte |

### 28.1 Metryki i sygnały już zbierane

Dokument opisuje obecnie zbierane sygnały w grupach, nie jako pełny katalog
Prometheus metric names. Zakres sygnałów obejmuje:

- Gatekeeper phase metrics: tx count, buy count, unique signers, phases passed,
  hard/soft reason chain, `verdict_type`, `reason_code`.
- Tx-intelligence metrics: buy/sell count, buy ratio, SOL buy ratio, volume CV,
  HHI, volume Gini, top3 volume, signer ratios, interval CV, timing entropy,
  burst ratio, failed tx count, dust tx count.
- Dev metrics: dev buy SOL, dev volume ratio, dev tx ratio, dev sold,
  dev-first-buyer and dev paperhand signals.
- Early fingerprint metrics: block0 sniped supply, flip ratio, CU price p90,
  priority fee surge slope, buyer pre-balance CV, inner ix average, CPI depth,
  sell/buy ratio, CU cluster dominance, static fee profile, fixed-size buys,
  flipper presence, Jito tip intensity, early-slot dominance, whale reversal.
- Sybil metrics: CPV, FSC, FTDI, DBIA, SFD, DES oraz degraded reasons.
- V2.5 metrics: DOW stage decisions, TAS score and sub-scores, PDD diagnostics,
  APS regime/suggestions, V2.5 confidence and availability.
- Shadow execution metrics: decision-to-sim latency, shadow duration, retry
  count, units consumed, RPC slot, simulated token delta fallback, error class,
  lifecycle `submitted/closed/failed/abandoned`, no-dispatch classifications.
- Logging/replay metrics: decision plane, rollout profile, config hash,
  schema version, terminal reason code completeness, shadow lifecycle
  reconciliation and report gates.

### 28.2 False BUY, false REJECT, timeout i degraded states

Dokument rozróżnia te przypadki jako kategorie analityczne:

- Potencjalny false BUY: BUY przeszedł Gatekeeper/IWIM, ale shadow simulation
  failuje, lifecycle nie zamyka się, post-buy synthetic outcome jest słaby,
  albo późniejsza analiza pokazuje PDD/TAS/sybil degraded evidence.
- Potencjalny false REJECT: reject wynika z brakującego albo zdegradowanego
  evidence, zbyt agresywnego progu, stale curve/account update, zbyt krótkiego
  okna albo konfiguracji shadow/live, która nie odpowiada realnemu reżimowi.
- Timeout: osobny terminalny stan, nie neutralny discard; może oznaczać
  `TIMEOUT_PHASE1_NO_DATA`, `TIMEOUT_PHASE1_INSUFFICIENT` albo
  `TIMEOUT_DEADLINE_LOW_PHASES`.
- Degraded state: brak danych fingerprint/sybil/PDD/TAS/curve jest oddzielnym
  stanem audytowym i nie powinien być liczony jako czysty pass.

### 28.3 Latency i observation window

Najważniejsze ograniczenia czasowe:

- Gatekeeper long-mode ma 10-sekundowe okno terminalne w aktualnym configu.
- DOW dzieli ocenę na Early 2-5 s, Normal 7 s i Extended 10 s.
- Krótkie okno oznacza, że jakość timestampów, slot quality, account update
  relay, session buffering i orphan tx replay mają bezpośredni wpływ na
  false BUY/false REJECT.
- `dow_tick_ms=250` daje regularne checkpointy, ale ticki są skipowane przy
  opóźnieniach, żeby nie spiętrzać zaległych ocen.
- Shadow-burnin mierzy decision-to-sim latency i shadow duration, ale nie
  dowodzi live inclusion ani realnego priority-fee contention.
- Jeśli AccountUpdate/curve readiness przychodzi za późno, BUY może przejść w
  `PendingCurve`, timeout albo reject zależnie od `curve_wait_ms` i
  `stale_fallback`.
