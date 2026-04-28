# POST-BUY LOGIC — Pełna Dokumentacja Flow Zarządzania Pozycją

**Data:** 2026-04-07  
**Autor:** Ghost Father  
**Status:** Audit zakończony — problemy zidentyfikowane  

---

## Spis treści

1. [Architektura ogólna](#1-architektura-ogólna)
2. [Ścieżki wejścia: jak pozycja trafia do Post-Buy](#2-ścieżki-wejścia)
3. [PostBuyRuntime (ghost-launcher) — Live Sell Lifecycle](#3-postbuyruntime)
4. [PostBuy Guardian / MonitoringEngine (ghost-brain)](#4-postbuy-guardian)
5. [4 moduły monitoringu: LIGMA, WHF, TCF, PANIC](#5-moduły-monitoringu)
6. [SignalRouter — most sygnałów do Revolvera](#6-signalrouter)
7. [Revolver — strategia wykonania SELL](#7-revolver)
8. [AEM (Adaptive Exit Manager) — uczący się decydent](#8-aem)
9. [PaperPositionLifecycle — papierowy cykl życia](#9-paperpositionlifecycle)
10. [LiveBackend / LiveBackendWorker — niskopoziomowa egzekucja](#10-livebackend)
11. [Pipeline Integration Map](#11-pipeline-integration-map)
12. [Konfiguracja produkcyjna](#12-konfiguracja)
13. [KRYTYCZNE PROBLEMY — dlaczego zarządzanie pozycją nie działa](#13-problemy)
14. [Rekomendacje naprawy](#14-rekomendacje)

---

## 1. Architektura ogólna

Flow Post-Buy składa się z **dwóch równoległych, niepowiązanych systemów zarządzania pozycją**:

```
                    ┌──────────────────────────────────────────────────┐
                    │              PostBuySubmitted Event              │
                    └───────────────┬──────────────────────────────────┘
                                    │
                    ┌───────────────▼───────────────┐
                    │       PostBuyRuntime           │
                    │   (ghost-launcher adapter)     │
                    └───────┬───────────────┬───────┘
                            │               │
                   lane="live"       lane="paper"
                            │               │
            ┌───────────────▼──┐    ┌───────▼──────────────┐
            │ run_live_sell_   │    │ PaperPositionLifecycle│
            │   lifecycle()    │    │  (AEM only, no LIGMA/ │
            │                  │    │   WHF/TCF/PANIC)      │
            │ FIXED +2%/-2%   │    └───────────────────────┘
            │ TP/SL only      │
            │ NO Guardian     │
            │ NO AEM          │
            └─────────────────┘

        ┌──────────────────────────────────────────────────┐
        │             MonitoringEngine (Guardian)           │
        │   (ghost-brain/src/guardian/post_buy/engine.rs)   │
        │                                                    │
        │   Tick loop: LIGMA → WHF → TCF → PANIC → AEM     │
        │                                                    │
        │   Rejestracja: pipeline/execution.rs (Paper)       │
        │                pipeline/jito_processor.rs (Jito)   │
        │                                                    │
        │   ⚠️ NIE wywoływana z LiveBackendWorker            │
        │   (fire_magazine_creation nie rejestruje pozycji) │
        └──────────────────────────┬───────────────────────┘
                                   │
                    ┌──────────────▼──────────────┐
                    │   SignalRouter / AEM v1      │
                    │   (jeśli aem.enabled=false   │
                    │    → SignalRouter do Revolver │
                    │    jeśli aem.enabled=true     │
                    │    → AEM tick w engine)       │
                    └──────────────┬──────────────┘
                                   │
                    ┌──────────────▼──────────────┐
                    │       Revolver               │
                    │  (off-chain/trigger)          │
                    │  Bullets, StrategyMode,       │
                    │  PanicSell, TightStopLoss     │
                    └──────────────────────────────┘
```

**Kluczowe odkrycie:** Live sell lifecycle (PostBuyRuntime) i Guardian/AEM/Revolver to **dwa oddzielne systemy**, które nie współpracują ze sobą w ścieżce live.

---

## 2. Ścieżki wejścia

### 2.1 Jak pozycja trafia do Post-Buy

Istnieją **trzy ścieżki rejestracji** pozycji w systemie post-buy:

| Ścieżka | Plik źródłowy | Guardian registration | Revolver magazine | PostBuyRuntime event |
|---------|---------------|----------------------|-------------------|---------------------|
| Paper mode | `pipeline/execution.rs` L852 | ✅ `register_position_with_context()` | ❌ (brak) | ✅ `PostBuySubmitted` (paper lane) |
| Jito batch | `pipeline/jito_processor.rs` L473 | ✅ `register_position_with_context()` | ✅ `load_magazine()` | ✅ `PostBuySubmitted` (live lane) |
| Standard live | `execution/live.rs` L522 | ❌ **NIE REJESTRUJE** | ✅ `fire_magazine_creation()` | ✅ (via event bus) |

### 2.2 Event PostBuySubmitted

Event `GhostEvent::PostBuySubmitted` zawiera:
- `candidate_id`, `pool_amm_id`, `base_mint`, `signature`
- `amount_sol`, `tip_lamports`, `lane` ("live" | "paper")
- `position_slot_id`, `source` (Normal | Recovery)
- `min_tokens_out`, `buy_landed_slot`, `creator_pubkey`

`PostBuyRuntime.run()` (linia 1359) nasłuchuje na ten event przez:
1. `EventBusReceiver` — główny kanał eventów
2. `DirectPostBuyReceiver` — bezpośredni kanał (fallback)

---

## 3. PostBuyRuntime

**Plik:** `ghost-launcher/src/components/post_buy_runtime.rs` (~3200 linii)  
**Rola:** Thin adapter z event bus do lifecycle Management pozycji  

### 3.1 Architektura wewnętrzna

```
PostBuyRuntimeConfig
├── live_sell_handle: LiveSellHandle
│   ├── rpc_client: AsyncRpcClient
│   ├── payer: Keypair
│   ├── live_tx_sender: HeliusSender (Jito + Yellowstone confirms)
│   ├── account_state_core: AccountStateReducer (canonical live prices)
│   └── shadow_ledger: ShadowLedger (diagnostic compare only)
├── paper_lifecycle: Arc<PaperPositionLifecycle>
├── position_limit_tracker: PositionLimitTracker (bulkhead)
├── live_position_registry: LivePositionRegistry (recovery/persistence)
└── sell_slippage_bps: u16
```

### 3.2 Dedup cache

`RecentPostBuyCache` — TTL-based dedup cache (64 entries). Zapobiega duplikatom `PostBuySubmitted` events na podstawie `candidate_id`. Counter: `post_buy_runtime_duplicate_handoff_total`.

### 3.3 Routing live vs paper

W `handle_post_buy_event()` (linia ~1500):

```
if lane == "live" → spawn run_live_sell_lifecycle()
if lane != "live" → paper_lifecycle.run()
```

### 3.4 Live Sell Lifecycle — DOKŁADNY OPIS

**Najważniejsza funkcja:** `run_live_sell_lifecycle_inner()` → `initialize_live_exit_session()` + `monitor_live_exit_session()`

#### Faza 1: Inicjalizacja (`initialize_live_exit_session`)

1. **Query wallet position** — `query_live_wallet_position_with_retry()`:
   - Próbuje Token-2022 program (nowe pump.fun minty od Q4-2025), potem legacy SPL
   - Max retries: `LIVE_SELL_ATA_LOOKUP_MAX_RETRIES`
   - Timeout per attempt: `LIVE_SELL_ATA_LOOKUP_RETRY_MS`

2. **Resolve entry price** — oblicza entry price z `buy_signature` + `tokens_received`

3. **Obliczanie TP/SL bands:**
   ```rust
   upper_exit_price = entry_price * 102 / 100  // +2% Take Profit
   lower_exit_price = entry_price * 98 / 100   // -2% Stop Loss
   ```
   Stałe hardcoded:
   - `LIVE_EXIT_TAKE_PROFIT_NUMERATOR = 102`
   - `LIVE_EXIT_STOP_LOSS_NUMERATOR = 98`
   - `LIVE_EXIT_THRESHOLD_DENOMINATOR = 100`

4. **Rejestracja w LivePositionRegistry** (opcjonalnie, dla recovery)

5. **Przejście stanowe:** `Armed → Monitoring`

#### Faza 2: Monitoring Loop (`monitor_live_exit_session`)

Pętla z interwałem `LIVE_SELL_POLL_MS`:

1. **Read price sample** — kaskadowy price feed:
   - Priorytet 1: `try_canonical_live_price()` → `AccountStateReducer.get_canonical_state()` (Yellowstone gRPC)
   - Priorytet 2: `read_price_from_rpc_point_query()` → bezpośredni RPC do bonding curve (pump.fun + bonk)
   - Fallback: `unavailable` (po `LIVE_EXIT_MONITORING_UNAVAILABLE_MAX_POLLS` → `MonitoringUnavailable`)

2. **Diagnostic shadow compare** — porównuje canonical price z ShadowLedger (tylko diagnostyka, NIE wpływa na decyzje)

3. **Record price sample** — aktualizuje peak/trough w sesji

4. **Snapshot metrics** — co 1s loguje snapshot + odpytuje wallet position (best-effort)

5. **Exit trigger check:**
   ```rust
   fn determine_live_exit_trigger(session, current_price) -> Option<LiveExitTrigger> {
       if current_price <= lower → StopLoss
       if current_price >= upper → TakeProfit
       else → None
   }
   ```

#### Faza 3: Wykonanie SELL (gdy trigger)

1. **Build transaction** — `build_full_exit_transaction_with_retry()`:
   - Pobiera curve execution hints (cashback, real_sol_reserves)
   - Oblicza `min_output` z SellTxBuilder + cappuje do real SOL reserves
   - Dynamic tip via `live_tx_sender.raise_tip_to_dynamic_floor()`
   - Dynamic priority fee via `estimate_priority_fee_micro_lamports()`
   - Max retries: `LIVE_EXIT_BUILD_MAX_RETRIES`
   - Blockhash reuse protection

2. **Pre-flight simulation** — `simulate_transaction()` (non-aborting, SELL proceed regardless)

3. **Submit** — `live_tx_sender.send_transaction()` (Helius Sender → Jito bundle)

4. **Confirm** — race between:
   - Yellowstone gRPC `confirm_submission()`
   - On-chain fallback: `confirm_sender_sell_via_fallback()`:
     - Token balance check (zero = sold)
     - Wallet position absent check
     - Signature status check
     - Poll interval: 250ms, timeout: 12s

5. **Retries** — jeśli `ExitSubmitFailed` lub `ExitConfirmFailed`:
   - Max: `LIVE_EXIT_EXECUTION_MAX_RETRIES`
   - Exponential backoff via `live_exit_retry_delay_ms()`

#### Stany LiveExitSession

```
Initialized → Armed → Monitoring → ExitTriggeredTakeProfit/ExitTriggeredStopLoss
  → ExitBuildFailed (terminal) | ExitSubmitFailed (retryable) | ExitConfirmFailed (retryable)
  → ExitConfirmed (terminal success)
  → MonitoringUnavailable (terminal — price unavailable too long)
  → LifecycleAbortedWithReason (terminal)
```

### 3.5 Metryki PostBuyRuntime

| Metryka | Opis |
|---------|------|
| `post_buy_runtime_duplicate_handoff_total` | Duplikaty PostBuySubmitted |
| `post_buy_live_exit_status_total` | Stany lifecycle per status |
| `post_buy_live_exit_trigger_total` | Triggery TP/SL |
| `post_buy_live_exit_terminal_total` | Terminalne stany per reason |
| `post_buy_live_exit_retry_total` | Retry count per status |
| `post_buy_price_source_total` | Źródło ceny (canonical/rpc/unavailable) |
| `live_exit_blockhash_fetch_latency_ms` | Latencja blockhash RPC |
| `live_exit_blockhash_to_send_transaction_ms` | Czas od blockhash do submit |
| `post_buy_live_sell_ata_resolution_failed_total` | Błędy ATA lookup |
| `post_buy_live_slot_retained_total` | Sloty niezwolnione (pozycja potencjalnie otwarta) |

---

## 4. PostBuy Guardian / MonitoringEngine

**Pliki:** `ghost-brain/src/guardian/post_buy/`
- `engine.rs` (~1100 linii) — MonitoringEngine: tick-based position monitor
- `signals.rs` (~250 linii) — typy sygnałów
- `integration.rs` (~280 linii) — SignalRouter
- `config.rs` (~200 linii) — PostBuyGuardianConfig

### 4.1 MonitoringEngine — serce Guardiana

```rust
pub struct MonitoringEngine {
    config: PostBuyGuardianConfig,
    shadow_ledger: Arc<ShadowLedger>,
    revolver: Option<Arc<RwLock<Revolver>>>,
    signal_tx: mpsc::Sender<GuardianSignal>,
    positions: HashMap<Pubkey, MonitoredPosition>,
    aem_runtime: Option<Arc<Mutex<AemRuntime>>>,
    aem_ledger: Option<Arc<dyn AemLedgerIo>>,
    event_emitter: Option<Arc<EventEmitter>>,
    secondary_event_emitter: Option<Arc<EventEmitter>>,
}
```

### 4.2 Struktura per-position

```rust
struct MonitoredPosition {
    pool_amm_id: Pubkey,
    bonding_curve: Pubkey,
    registered_at: Instant,
    entry_price: Option<f64>,
    amount_lamports: Option<u64>,
    context: Option<PositionEventContext>,
    tcf: Option<TcfField>,           // TCF per-position instance
    tradability: Option<f64>,        // Last LIGMA tradability
    signals: Vec<TimedGuardianSignal>,
    aem_position_id: Option<String>,
    aem_position_epoch: u64,
}
```

### 4.3 Tick loop — `engine.start()` → `tick()`

Interwał: `tick_interval_ms` (default: 500ms)

Dla każdego monitorowanego minta, per tick:

```
1. run_ligma_check(mint)     → severity: Info/Warning/Critical
2. run_whf_check(mint)       → severity: Info/Warning/Critical  
3. run_tcf_check(mint)       → severity: Info/Warning/Critical
4. run_panic_check(mint)     → severity: Info/Warning/Critical
5. cleanup_old_signals(mint) → usuwanie przeterminowanych sygnałów
6. run_aem_tick(mint)        → AEM decision loop (jeśli aem.enabled)
7. sync_with_revolver()      → usuwanie pozycji których nie ma w Revolver
```

### 4.4 Rejestracja pozycji

```rust
// Pełna rejestracja z kontekstem
fn register_position_with_context(
    pool_amm_id: Pubkey,   // AMM pool
    base_mint: Pubkey,      // Token mint (klucz HashMap)
    bonding_curve: Pubkey,  // Bonding curve PDA
    entry_price: Option<f64>,
    amount_lamports: Option<u64>,
    context: Option<PositionEventContext>,
) -> Option<&MonitoredPosition>
```

- Sprawdza `max_monitored_positions` limit
- Jeśli mint już istnieje → aktualizuje zamiast duplikować
- Inicjalizuje TCF field per-position
- Zwraca referencję do zarejestrowanej pozycji

### 4.5 Agregacja sygnałów → PositionHealth

```rust
fn get_position_health(mint: &Pubkey) -> Option<PositionHealth> {
    // Zbiera sygnały z okna signal_aggregation_window_ms (5s)
    // Oblicza: health_score, liquidity_tradability, trend_cohesion,
    //          manipulation_detected, panic_impulse_active
    // Wyznacza recommended_action via compute_recommendation()
}

fn compute_recommendation(signals, config) -> RecommendedAction {
    if critical_count >= escalation_critical_count (1) || panic_present → PanicSell
    if manipulation_detected → DefensiveMode  
    if warning_count >= escalation_warning_count (3) → TightenStop
    else → Hold
}
```

---

## 5. Moduły monitoringu: LIGMA, WHF, TCF, PANIC

### 5.1 LIGMA — Liquidity Impact & Grant Market Analysis

**Cel:** Monitoruje głębokość liquidity, wykrywa pułapki płynnościowe.

**Implementacja w Guardian** (uproszczona vs pełny moduł):
```rust
fn run_ligma_check(mint: Pubkey) {
    // Pobiera snapshot z ShadowLedger
    let snapshot = shadow_ledger.get_snapshots(&bonding_curve);
    
    // Oblicza impact w BPS (constant-product formula):
    // impact_bps = (probe_sol / virtual_sol_reserves) * 10000
    
    // Oblicza tradability:
    // tradability = min(1.0, real_sol_reserves / (probe_sol * SCALE))
    
    // Klasyfikacja:
    if impact_bps >= ligma_critical_impact_bps(8000) || tradability <= ligma_critical_tradability(0.15)
        → Critical
    if impact_bps >= ligma_warning_impact_bps(3500) || tradability <= ligma_warning_tradability(0.4)
        → Warning
    else → Info (no signal)
}
```

**Domyślne progi:**
- Warning impact: 3500 BPS (35%), Critical: 8000 BPS (80%)
- Warning tradability: 0.4, Critical: 0.15
- Probe SOL: 0.1 SOL

**Uwaga:** Pełny LIGMA (`ghost-brain/src/signals/ligma.rs`) używa AmmPool simulation z SimulatedSwap — Guardian ma tylko uproszczoną wersję z constant-product formula.

### 5.2 WHF — Wash Harmonic Field

**Cel:** Wykrywa wash trading i manipulację wolumenem.

**Implementacja w Guardian:**
```rust
fn run_whf_check(mint: Pubkey) {
    // Pobiera snapshot z ShadowLedger
    
    // Analiza:
    // 1. Net flow SOL = buy_volume - sell_volume
    //    Jeśli |net_flow| < whf_min_net_flow_sol(0.5) → podejrzane
    //
    // 2. Price change relative = |price_now - price_prev| / price_prev
    //    Jeśli price_change < whf_wash_max_price_change(0.02) 
    //    ORAZ volume jest wysoki → wash trading
    //
    // Klasyfikacja:
    if whf_wash_trading_is_critical(true) && wash_detected → Critical
    else if wash_detected → Warning
}
```

**Domyślne progi:**
- Min net flow SOL: 0.5
- Wash max price change: 0.02 (2%)
- WHF min confidence: 0.6
- Wash trading is critical: true

**Uwaga:** Pełny WHF (`ghost-brain/src/chaos/whf_signals.rs`) używa HarmonicFieldAnalysis z FFT — Guardian ma uproszczoną analizę wolumen/cena.

### 5.3 TCF — Trend Cohesion Field

**Cel:** Wykrywa utratę koherencji trendu, cliff events (nagłe załamania).

**Implementacja w Guardian:**
```rust
fn run_tcf_check(mint: Pubkey) {
    // Używa per-position TcfField instance
    // Aktualizuje pole z nowymi danymi z ShadowLedger
    
    // tcf_field.update(snapshot) → oblicza cohesion score [0.0, 1.0]
    // Konsekutywne niskie odczyty liczone w consecutive_low_count
    
    // Klasyfikacja:
    if cohesion <= tcf_critical_cohesion(0.2) → Critical
    if cohesion <= tcf_warning_cohesion(0.4) || consecutive_low >= tcf_consecutive_low_max(5)
        → Warning
    if cliff_detected && tcf_cliff_is_warning → Warning
}
```

**Domyślne progi:**
- Warning cohesion: 0.4, Critical: 0.2
- Cliff is warning: true
- Consecutive low max: 5

**Uwaga:** Pełny TCF (`ghost-brain/src/oracle/tcf/cohesion.rs`) ma zaawansowany model koherencji pola — Guardian używa uproszczonej wersji.

### 5.4 PANIC — Congestion & Coordinated Sell Detection

**Cel:** Wykrywa skoki TX rate i skoordynowane sell-offy (niska entropia nadawców).

**Implementacja w Guardian:**
```rust
fn run_panic_check(mint: Pubkey) {
    // Pobiera recent transactions z ShadowLedger
    // Mierzy TX/s w oknie panic_rate_window_ms (2s)
    // Oblicza entropy nadawców (Shannon entropy)
    
    // Klasyfikacja:
    if txps >= panic_critical_txps(30.0) → Critical
    if txps >= panic_warning_txps(15.0) → Warning
    
    // Dodatkowy check: 
    if entropy < panic_low_entropy_threshold(1.0) && txps >= panic_warning_txps
        → Critical (skoordynowany atak — wielu TX z nielicznych adresów)
}
```

**Domyślne progi:**
- Warning TX/s: 15.0, Critical: 30.0
- Low entropy threshold: 1.0
- Rate window: 2000ms

**Uwaga:** Pełny PANIC (`ghost-brain/src/oracle/ultrafast/panic.rs`) używa CIR-scaled density + PanicState machine z 0-7s window — Guardian ma uproszczoną wersję.

---

## 6. SignalRouter — most sygnałów do Revolvera

**Plik:** `ghost-brain/src/guardian/post_buy/integration.rs`

### 6.1 Kiedy aktywny

SignalRouter jest uruchomiony **TYLKO gdy `aem.enabled = false`**:
```rust
// pipeline/builder.rs
if !aem_enabled {
    let router = SignalRouter::new(signal_rx, revolver_clone);
    tokio::task::spawn(router.run());
}
```

Gdy `aem.enabled = true` (domyślny stan produkcyjny), SignalRouter NIE jest uruchomiony. Sygnały z 4 modułów są nadal wysyłane do `signal_tx`, ale nikt ich nie konsumuje (poza `run_aem_tick` w engine, który nie czyta z kanału).

### 6.2 Mapowanie sygnał → strategia Revolvera

```
GuardianSignal(Critical, any source) → PanicSell
  → revolver.set_panic_sell(mint)

GuardianSignal(Warning, WHF) → DefensiveMode  
  → revolver.set_tight_stop_loss(mint)

GuardianSignal(Warning, LIGMA/TCF/PANIC) → TightenStop
  → revolver.set_tight_stop_loss(mint)

GuardianSignal(Info, any) → Hold
  → no-op
```

### 6.3 Reguły eskalacji

- **Jednokierunkowa:** PanicSell → nigdy nie downgrade do TightStopLoss
- **Idempotentna:** powtórne ustawienie tego samego trybu = no-op
- **Natychmiastowa:** brak debounce, każdy sygnał działa natychmiast

---

## 7. Revolver — strategia wykonania SELL

**Plik:** `off-chain/components/trigger/src/revolver.rs` (~800 linii)

### 7.1 Hierarchia

```rust
struct Bullet {
    tx_bytes: Vec<u8>,          // Zserializowana SELL tx
    target_price: u64,          // lamports/token
    position_fraction_bps: u16, // część pozycji (10000 = 100%)
    time_stop: Option<u64>,     // Unix timestamp deadline
    requeue_count: u32,         // Ile razy ponowiono
}

struct TokenRevolver {
    mint: Pubkey,
    bullets: Vec<Bullet>,
    quarantined_bullets: Vec<QuarantinedBullet>,
    strategy_mode: StrategyMode,  // Default/PanicSell/TightStopLoss/LooseStopLoss
    position_epoch: u64,
    force_exit_all: bool,
    force_exit_fraction_bps: Option<u16>,
    aem_command: Option<ActiveAemCommand>,
}

struct Revolver {
    magazines: HashMap<Pubkey, TokenRevolver>,
}
```

### 7.2 StrategyMode

```
Default       → standardowe pola cenowe bullet targets
PanicSell     → check_targets() → fire ALL bullets natychmiast
TightStopLoss → obniża target price (tighter exit)
LooseStopLoss → poluzowuje target price (looser exit)
```

### 7.3 `check_targets()` — logika strzelania

```rust
fn check_targets(current_price, now) {
    if force_exit_all || strategy_mode == PanicSell {
        → fire ALL bullets (return all tx_bytes)
    }
    
    if force_exit_fraction_bps set {
        → fire bullets up to fraction
    }
    
    for each bullet:
        if current_price >= target_price → fire (TakeProfit)
        if current_price <= stop_loss_price(strategy) → fire (StopLoss)
        if time_stop expired → fire (TimeStop)
}
```

### 7.4 AEM Command Support

```rust
fn apply_aem_control_command(
    position_id, epoch, issued_at, valid_from, expires_at,
    priority, directive, reason_code, now
) → CommandApplyResult {
    // Epoch validation: odrzuca jeśli epoch nie pasuje (anty-zombie)
    // Time window: valid_from <= now <= expires_at
    // Hard safety lock: HardSafety priority nadpisuje wszystko
    // Directive application:
    //   ForceExitAll → force_exit_all = true
    //   ForceExitFractionBps → force_exit_fraction_bps = Some(bps)
    //   SetTightStop → strategy_mode = TightStopLoss
    //   SetLooseStop → strategy_mode = LooseStopLoss
    //   FreezePanic → strategy_mode = Default (reset)
    //   Noop → no change
}
```

### 7.5 Sync z Guardinaem

`MonitoringEngine.sync_with_revolver()`:
```rust
fn sync_with_revolver() {
    let active_mints = revolver.get_active_mints();
    for monitored_mint in self.positions.keys() {
        if !active_mints.contains(monitored_mint) {
            // Mint opuścił Revolver → usuń z monitoringu
            self.unregister_position(monitored_mint);
        }
    }
}
```

---

## 8. AEM — Adaptive Exit Manager

**Pliki:** `ghost-brain/src/aem/runtime.rs`, `types.rs`, `feature_adapter.rs`

### 8.1 Architektura AEM

```rust
struct AemRuntime {
    config: AemConfig,
    hard_safety: Vec<Box<dyn HardSafetyCheck>>,
    policy: PolicyEngine,
    regime_book: RegimeBook,   // stats per (RegimeKey, ActionChosen)
}
```

### 8.2 Decision pipeline: `on_tick_with_report()`

```
1. Hard Safety Check
   → Evaluate each HardSafetyCheck against StateFeatures
   → SafetyAction::ImmediateSell → ForceExitAll (HardSafety priority)
   → SafetyAction::FreezePosition → FreezePanic
   
2. Policy Engine Decision
   → policy.decide(features, regime_book, config) → PolicyDecision
   → ActionChosen: SellNow / WaitReclaim / Partial / Panic
   → CI bounds validation (confidence intervals)
   
3. Build ControlCommand
   → choose_default_directive(action, partial_fraction_bps)
   → ControlCommand { position_id, action, directive, epoch, priority, validity }
   
4. Apply via TriggerControlAdapter
   → trigger.apply_control_command(cmd, now)
   → Result: accepted / rejected (with reason)
   
5. Record ManagementDecisionEvent to JSONL ledger
```

### 8.3 StateFeatures — dane wejściowe

```rust
struct StateFeatures {
    drawdown_pct: f64,          // Max drawdown od entry
    pnl_pct: f64,               // Bieżący PnL
    slope_pct_per_s: f64,       // Nachylenie ceny
    stress_bucket: StressBucket, // Low/Medium/High
    reclaim_flag: bool,         // Czy cena odbiła z low
    time_held_s: u64,           // Czas od entry
    // ... dodatkowe pola
}
```

### 8.4 RegimeBook — uczenie counterfactual

`flush_due_outcomes()`:
- Czyta decisions_without_outcome z ledgera
- Jeśli outcome window minął → oblicza counterfactual_delta_pnl
- Aktualizuje `ActionStats` per `(RegimeKey, ActionChosen)`:
  - n (count), mean_delta_pnl, std_delta_pnl, tail_risk_rate
- CI bounds → PolicyEngine używa UCB/LCB do wyboru akcji

### 8.5 RolloutMode

```
Shadow      → AEM generuje decyzje, ale NIE aplikuje do Revolvera (counterfactual only)
PilotLive   → AEM aplikuje TYLKO gdy drawdown >= 40% && stress == Low
FullLive    → AEM aplikuje zawsze (wymaga positive mean_delta && low tail risk)
```

Promocja na wyższy rollout level zależy od ordinal (ilość ticków AEM) i warunków.

### 8.6 RevolverAemAdapter

```rust
// feature_adapter.rs
impl TriggerControlAdapter for RevolverAemAdapter {
    fn apply_control_command(cmd, now) {
        // Mapuje AEM CommandDirective → Revolver AemCommandDirective
        // Deleguje do revolver.apply_aem_control_command()
    }
}
```

### 8.7 AEM w Guardian vs AEM w PaperLifecycle

Guardian's `run_aem_tick()` (engine.rs):  
- Buduje `StateFeatures` z ShadowLedger snapshot 
- Wywołuje `aem_runtime.on_tick_with_report()` per position  
- Wynik: `ManagementDecisionEvent` + `ControlCommand` → Revolver

PaperPositionLifecycle (paper_lifecycle.rs):
- Ma ODDZIELNĄ instancję `AemRuntime`  
- Buduje `StateFeatures` z synthetic_mark_price (PaperBroker)
- Wynik: `ManagementDecisionEvent` → BridgeTriggerAdapter

**Te dwie instancje AEM są niezależne** — mają oddzielne RegimeBooki i ledgery.

---

## 9. PaperPositionLifecycle

**Plik:** `ghost-brain/src/execution/paper_lifecycle.rs`

### 9.1 Opis

Self-driving paper position lifecycle:
- Używa `PaperBroker` do synthetic execution
- `BridgeTriggerAdapter` przechwytuje AEM commands
- **NIE używa LIGMA/WHF/TCF/PANIC** — tylko AEM
- **NIE używa Guardiana** bezpośrednio (ale Guardian monitoruje tę samą pozycję przez osobną rejestrację)
- Cena z `synthetic_mark_price` (model cenowy papierowy)

### 9.2 Lifecycle

```
run() → loop {
    tick AEM runtime → decision
    BridgeTriggerAdapter.apply_control_command()
    if ForceExitAll → execute paper sell
    report metrics
}
```

---

## 10. LiveBackend / LiveBackendWorker

**Plik:** `ghost-brain/src/execution/live.rs` (~750 linii)

### 10.1 Rola

Niskopoziomowy backend dla live execution: RPC transaction send + Revolver magazine management.

### 10.2 Dwie pętle

```rust
enum Loop {
    StandardLoop,  // Sekwencyjne RPC send + confirm (15s timeout)
    JitoLoop,      // Batch Jito via jito_executor
}
```

### 10.3 Standard Loop — flow

```
recv entry_rx → LiveEntryRequest
  → build DirectBuy instruction
  → get_latest_blockhash()
  → send_and_confirm_transaction() (15s timeout)
  → success: mark_fill_success() + fire_magazine_creation()
  → failure: mark_fill_failed()
```

### 10.4 fire_magazine_creation() — ⚠️ KRYTYCZNE

```rust
async fn fire_magazine_creation(req) {
    tokio::spawn(async {
        // Create magazine bullets (TP/SL/TimeStop levels)
        let bullets = create_magazine_after_buy(payer, mint, size, price, config, rpc);
        // Load into Revolver
        revolver.load_magazine(mint, bullets);
        // ⚠️ NIE REJESTRUJE W GUARDIAN!
    });
}
```

### 10.5 `submit_exit()` — emergency exit

```rust
fn submit_exit(position_id) {
    let mint = parse_mint_from_position_id(position_id);
    revolver.set_force_exit_all(mint);
    revolver.set_strategy_mode(mint, PanicSell);
}
```

---

## 11. Pipeline Integration Map

### Kto rejestruje pozycje w Guardian:

| Komponent | Plik | Rejestracja | Uwagi |
|-----------|------|-------------|-------|
| `process_paper_swap_plan` | `execution.rs:852` | ✅ `register_position_with_context` | Paper mode — działa |
| `process_jito_batch` | `jito_processor.rs:473` | ✅ `register_position_with_context` | Jito live — działa |
| `LiveBackendWorker` | `live.rs:522,580` | ❌ **BRAK** | Standard live — **NIE REJESTRUJE** |
| `PostBuyRuntime live sell` | `post_buy_runtime.rs` | ❌ **BRAK** | Osobny system — nie używa Guardiana |

### Kto czyta z Guardiana:

| Komponent | Cel | Aktywny? |
|-----------|-----|----------|
| SignalRouter | Mutuje Revolver strategy | ❌ gdy aem.enabled=true (prod) |
| AEM via engine.run_aem_tick | Generuje ControlCommands → Revolver | ✅ (ale zależy od RolloutMode) |
| PostBuyRuntime | NIE czyta z Guardiana w ogóle | — |

### Kto strzela SELL:

| System | Trigger | Transport | Aktywny? |
|--------|---------|-----------|----------|
| PostBuyRuntime live_sell | Fixed +2%/-2% TP/SL | Helius Sender + Jito | ✅ (live lane) |
| Revolver check_targets | Bullet targets + strategy | revolver_worker → Jito | ✅ (ale potrzebuje magazine) |
| PaperLifecycle | AEM decision | Synthetic (paper) | ✅ (paper lane) |

---

## 12. Konfiguracja produkcyjna

**`ghost_brain_config.toml` sekcja `[post_buy_guardian]`:**

```toml
[post_buy_guardian]
enabled = true               # Guardian WŁĄCZONY
tick_interval_ms = 500        # 2 ticki/s
max_monitored_positions = 10  # Max 10 pozycji jednocześnie

# LIGMA
ligma_warning_impact_bps = 3500.0
ligma_critical_impact_bps = 8000.0
ligma_warning_tradability = 0.4
ligma_critical_tradability = 0.15
ligma_probe_sol = 0.1

# WHF  
whf_min_confidence = 0.6
whf_wash_trading_is_critical = true
whf_min_net_flow_sol = 0.5
whf_wash_max_price_change = 0.02

# TCF
tcf_warning_cohesion = 0.4
tcf_critical_cohesion = 0.2
tcf_cliff_is_warning = true
tcf_consecutive_low_max = 5

# PANIC
panic_warning_txps = 15.0
panic_critical_txps = 30.0
panic_low_entropy_threshold = 1.0
panic_rate_window_ms = 2000

# Escalation
escalation_warning_count = 3
escalation_critical_count = 1
signal_aggregation_window_ms = 5000

[post_buy_guardian.aem]
enabled = true                # AEM WŁĄCZONY
# ... 30+ parametrów kalibracyjnych
```

---

## 13. KRYTYCZNE PROBLEMY — dlaczego zarządzanie pozycją nie działa

### 🔴 P1: Live Sell Lifecycle KOMPLETNIE POMIJA Guardian (KRYTYCZNE)

**Lokalizacja:** `ghost-launcher/src/components/post_buy_runtime.rs`

**Problem:** `run_live_sell_lifecycle()` → `monitor_live_exit_session()` ma hardcoded +2%/-2% TP/SL i **w żaden sposób nie konsultuje** MonitoringEngine/LIGMA/WHF/TCF/PANIC. Live sell nie czyta żadnych sygnałów z Guardiana.

**Konsekwencja:** Pozycje na live lane mają **ZERO ochrony** przed:
- Wash tradingiem (WHF)
- Pułapkami płynnościowymi (LIGMA)
- Załamaniami trendu (TCF)
- Skoordynowanymi sell-offami (PANIC)

Jedyny trigger to prymitywne `price >= entry * 1.02` lub `price <= entry * 0.98`.

**Ścieżka naprawy:** Live sell lifecycle musi odpytywać Guardian `get_position_health()` i reagować na `RecommendedAction::PanicSell`/`DefensiveMode`/`TightenStop`.

---

### 🔴 P2: LiveBackendWorker NIE rejestruje pozycji w Guardian (KRYTYCZNE)

**Lokalizacja:** `ghost-brain/src/execution/live.rs:639` — `fire_magazine_creation()`

**Problem:** Gdy BUY trafia przez standard live loop (nie-Jito), `fire_magazine_creation()` ładuje magazine do Revolvera, ale **nigdy nie wywołuje `guardian.register_position_with_context()`**. Pomimo że `LiveBackendConfig.post_buy_guardian` jest `Some(engine)`.

**Porównanie:**
- `jito_processor.rs:473` → ✅ rejestruje w Guardian
- `execution.rs:852` (paper) → ✅ rejestruje w Guardian  
- `live.rs:639` (standard live) → ❌ **NIE rejestruje**

**Konsekwencja:** Pozycje otwierane przez standard live path są **niewidoczne** dla Guardian/AEM. Moduły LIGMA/WHF/TCF/PANIC nigdy ich nie monitorują.

---

### 🔴 P3: SignalRouter WYŁĄCZONY gdy AEM enabled (KRYTYCZNE)

**Lokalizacja:** `ghost-brain/src/pipeline/builder.rs:380`

**Problem:** W konfiguracji produkcyjnej `aem.enabled = true`. Kod:
```rust
if !aem_enabled {
    let router = SignalRouter::new(signal_rx, revolver_clone);
    tokio::task::spawn(router.run());
}
```

Gdy AEM jest włączony, SignalRouter **nie jest uruchomiony**. Sygnały Critical z LIGMA/WHF/TCF/PANIC trafiają do `signal_tx`, ale **nikt ich nie konsumuje**. Kanał mpsc zapełni się do `signal_channel_buffer=256` a potem `try_send` zwróci `Err(Full)`.

Guardian's `run_aem_tick()` bezpośrednio buduje `ControlCommand` z AEM, ale **nie czyta z signal_rx**. 4 moduły (LIGMA/WHF/TCF/PANIC) produkują sygnały, ale te sygnały **nie wpływają na AEM decisions** ani na Revolvera.

**Konsekwencja:** Gdy AEM jest włączony (produkcja), 4 moduły monitoringu są de facto **martwe** — produkują sygnały, które nikt nie odbiera.

---

### 🟠 P4: AEM w trybie Shadow — NIE wykonuje SELL (WYSOKIE)

**Lokalizacja:** `ghost-brain/src/aem/runtime.rs`

**Problem:** AEM używa `RolloutMode` z progresywną promocją:
- `Shadow` → generuje decyzje, **NIE aplikuje** do Revolvera
- `PilotLive` → aplikuje TYLKO gdy drawdown >= 40% && stress == Low
- `FullLive` → wymaga pozytywnego mean_delta pnl i niskiego tail risk

Nowy system AEM startuje w **Shadow mode** i potrzebuje zebrać wystarczającą ilość counterfactual data (`n_min_per_key = 20` events per regime) zanim przejdzie do PilotLive.

**Konsekwencja:** Dopóki AEM nie zbierze wystarczających danych, **żadne AEM-driven SELL nie zostanie wykonane**. W połączeniu z P3 (SignalRouter wyłączony), system nie ma żadnego inteligentnego exit mechanism.

---

### 🟠 P5: Dwie niezależne instancje AEM (ŚREDNIE)

**Problem:** Guardian's engine ma `run_aem_tick()` z jednym AEM runtime, a PaperPositionLifecycle ma swój oddzielny AEM runtime. Te instancje:
- Mają oddzielne `RegimeBook` (niezależne uczenie)
- Czytają z oddzielnych source'ów (ShadowLedger vs synthetic price)
- Mogą podejmować sprzeczne decyzje dla tej samej pozycji

**Konsekwencja:** Dual mode (live+paper) może generować conflicting exit signals.

---

### 🟠 P6: ShadowLedger fail-open (ŚREDNIE)

**Lokalizacja:** Wszystkie 4 moduły w `engine.rs`

**Problem:** Każdy moduł zaczyna od:
```rust
let snapshot = shadow_ledger.get_snapshots(&bonding_curve);
if snapshot.is_none() { continue; } // Silent skip → no signal
```

Jeśli ShadowLedger nie ma danych (stale/missing snapshot, gRPC disconnect), moduły **cicho skipują** tick bez generowania żadnego sygnału. Nie ma alarmu "data unavailable".

**Konsekwencja:** Blind spot — brak danych = brak ochrony. System wierzy że "wszystko OK" kiedy w rzeczywistości nie ma danych do analizy. Fail-open zamiast fail-safe.

---

### 🟡 P7: Hardcoded +2%/-2% bez trailing stop (ŚREDNIE)

**Lokalizacja:** `post_buy_runtime.rs:195-197`

**Problem:**
```rust
const LIVE_EXIT_TAKE_PROFIT_NUMERATOR: u64 = 102;  // +2%
const LIVE_EXIT_STOP_LOSS_NUMERATOR: u64 = 98;      // -2%
```

- Brak trailing stop loss (nie podnosi SL gdy cena rośnie)
- Brak dynamic TP/SL na podstawie volatility
- Brak adjustments based on market conditions
- 2% to ekstremalnie ciasny band na pump.fun tokens z typową volatility 50-200%

**Konsekwencja:** Pozycja z entry 1.0 SOL ma SL=0.98 SOL i TP=1.02 SOL. Na pump.fun token z normalnym price action, SL lub TP uderzy w ciągu sekund — praktycznie losowy wynik.

---

### 🟡 P8: Revolver quarantine leaks (NISKIE)

**Lokalizacja:** `revolver.rs` — `quarantined_bullets` + `sync_with_revolver()`

**Problem:** `cleanup_empty()` usuwa TokenRevolver tylko gdy `bullets.is_empty()`, ale nie sprawdza `quarantined_bullets`. Kwarantowane bullet'y trzymają mint w Revolver. Guardian's `sync_with_revolver()` sprawdza `get_active_mints()` — jeśli zwraca minty z quarantined bullets, pozycja nigdy nie zostanie auto-unregistered.

**Konsekwencja:** Memory leak — Ghost positions w Guardian monitoring, wypełniające `max_monitored_positions=10` limit i blokujące nowe pozycje.

---

### 🟡 P9: Guardian moduły to UPROSZCZONE reimplementacje (NISKIE)

**Problem:** Moduły w Guardian engine to uproszczone wersje pełnych modułów:

| Moduł | Pełna wersja | Guardian wersja |
|-------|-------------|-----------------|
| LIGMA | AmmPool simulation, SimulatedSwap | Constant-product formula |
| WHF | HarmonicFieldAnalysis, FFT | Volume/price ratio check |
| TCF | Full TcfField cohesion model | Simplified cohesion read |
| PANIC | CIR-scaled density, PanicState FSM | TX rate + Shannon entropy |

To design decision, nie bug — ale zmniejsza skuteczność wykrywania.

---

### 🟡 P10: Signal channel overflow (NISKIE)

**Problem:** Gdy AEM enabled (P3), `signal_tx` kanał zapełnia się do 256. `try_send` zwraca `Err(Full)` — sygnały są po cichu wyrzucane. Brak metryki na dropped signals.

---

## 14. Rekomendacje naprawy

### Priorytet 1 (P1+P2+P3) — naprawić TERAZ

1. **Zintegrować Guardian z PostBuyRuntime live sell:**
   - `monitor_live_exit_session()` musi odpytywać `guardian.get_position_health(mint)` per tick
   - `RecommendedAction::PanicSell` → natychmiastowy trigger SELL (bypass TP/SL check)
   - `RecommendedAction::DefensiveMode/TightenStop` → dynamicznie zacieśniać SL

2. **Dodać rejestrację pozycji w `fire_magazine_creation()`:**
   ```rust
   // Po revolver.load_magazine(mint, bullets):
   if let Some(ref guardian) = self.config.post_buy_guardian {
       guardian.register_position_with_context(pool_amm_id, mint, ...);
   }
   ```

3. **Naprawić routing sygnałów gdy AEM enabled:**
   - Opcja A: SignalRouter ZAWSZE aktywny, AEM jako dodatkowa warstwa
   - Opcja B: AEM musi czytać `get_position_health()` i uwzględniać 4 moduły w `StateFeatures`
   - Opcja C: Sygnały Critical z 4 modułów → bezpośredni `revolver.set_panic_sell()` (bypass AEM)

### Priorytet 2

4. **AEM bootstrap accelerator:**
   - Seed RegimeBook z historycznych danych zamiast startować od zera w Shadow mode
   - Lub: dopóki Shadow, fallback na SignalRouter-based exits

5. **ShadowLedger fail-CLOSED:**
   - Jeśli snapshot unavailable przez 3+ ticki → emit Warning signal
   - Jeśli unavailable przez 10+ ticki → emit Critical/PanicSell

6. **Trailing stop loss:**
   - Zastąpić hardcoded 102/98 dynamicznym trailing stop
   - Podnosić SL z każdym nowym high (np. ATR-based)
   - Rozszerzać TP band na podstawie momentum

### Priorytet 3

7. Metryka `guardian_signal_dropped_total` na overflow signal channel
8. Guardian moduły: dodać `data_unavailable_ticks_total` metrykę
9. Revolver: `cleanup_empty()` powinien uwzględniać quarantine
10. Unified AEM: jedna instancja shared między Guardian i PaperLifecycle

---

*Dokument wygenerowany na podstawie audytu kodu źródłowego. Wszystkie line references odnoszą się do stanu kodu na dzień generacji.*
