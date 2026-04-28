# PRZEPŁYW DECYZYJNY GHOST GATEKEEPER V2 - AUDIT KODOWY

**Status:** Tylko rzeczywisty kod, zero spekulacji.  
**Data:** 2026-04-25  
**Auditorium:** gatekeeper_policy.rs, post_buy_runtime.rs, oracle_runtime.rs, ghost_brain_config.toml

---

## ⚠️ KRYTYCZNE USTALENIA

### Konfiguracja vs Implementacja - ROZBIEŻNOŚCI:

| Parametr | Config | Kod | Status |
|----------|--------|-----|--------|
| exit_strategy.tp_phase1_target_pct | 12.0% | Nie używane | ❌ Config orphaned |
| exit_strategy.stop_loss_pct | -12.0% | Nie używane | ❌ Config orphaned |
| exit_strategy.tp_phase1_soft_stop_hold_ms | 45000ms | Nie używane | ❌ Config orphaned |
| live_exit_take_profit_pct (hardcoded) | N/A | 0.02 (2%) | ✅ Rzeczywisty |
| live_exit_stop_loss_pct (hardcoded) | N/A | 0.02 (2%) | ✅ Rzeczywisty |
| max_ticks_before_exit (hardcoded) | N/A | 240 @ 500ms = 120s | ✅ Rzeczywisty |

**WNIOSEK:** Sekcja `[exit_strategy]` w ghost_brain_config.toml jest **MARTWA KONFIGURACJA**. System faktycznie:
- Wychodzi na +2% (nie +12%)
- Zamyka na -2% (nie -12%)
- Trzyma max 120 sekund (nie 1800 sekund)

---

## 1. OKNO OBSERWACJI - GATEKEEPER PHASE COLLECTION

### Config (ghost_brain_config.toml, linia 19)
```toml
mode = "long"
max_wait_time_ms = 8000
```

### Kod (gatekeeper.rs, oracle_runtime.rs)
```rust
// oracle_runtime.rs:2493
let ab_window_ms: u64 = 10_000;  // ⭐ OKNO OBSERWACJI = 10000ms, NIE 2000ms

// gatekeeper_policy.rs - mode "long" oznacza:
// T0 → T+8000ms: zbiera TX bez wczesnych decyzji
// T+8000ms: ONE evaluation
```

**Rzeczywistość:**
- **ab_window_ms = 10000ms** (dane się zbierają przez 10 sekund)
- **max_wait_time_ms = 8000ms** (decyzja gatekeeper pada po 8 sekund)
- **Strata danych:** Co się dzieje między sekundą 8 a sekundą 10? Nieznane.

---

## 2. PHASE 1-6 EWALUACJA - DEADLINE (T+8000ms)

### Kod (gatekeeper_policy.rs, linie 275-343)

```rust
pub fn build_assessment_from_features(
    features: MaterializedFeatureSet,
    config: &GatekeeperV2Config,
    context: PolicyEvaluationContext,
) -> GatekeeperAssessment {
    
    // PHASE 1: HARD REQUIREMENT
    let phase1_passed = features.tx_intel_features.tx_count >= config.min_tx_count as u64
        && features.tx_intel_features.unique_signers >= config.min_unique_signers as u64
        && features.tx_intel_features.buy_count >= config.min_buy_count as u64;

    // PHASE 2: Velocity
    let phase2_velocity = velocity_profile_from_features(&features);
    let phase2_passed = phase2_velocity.map(|v| {
        v.avg_interval_ms >= config.min_avg_interval_ms
            && v.avg_interval_ms <= config.max_avg_interval_ms
            && v.timing_entropy >= config.min_timing_entropy
    }).unwrap_or(false);

    // PHASE 3: Diversity (HHI, Gini)
    let phase3_diversity = signer_diversity_from_features(&features);
    let phase3_passed = phase3_diversity.map(|d| diversity_phase_passes(d, config)).unwrap_or(false);

    // PHASE 4: Volume  
    let phase4_volume = volume_sanity_from_features(&features);
    let phase4_passed = phase4_volume.map(|v| volume_phase_passes(v, None, config)).unwrap_or(false);

    // PHASE 5: Dev Behavior
    let phase5_dev = dev_behavior_from_features(&features);
    let phase5_passed = phase5_dev.map(|d| {
        d.dev_volume_ratio <= config.max_dev_volume_ratio
            && d.dev_tx_ratio <= config.max_dev_tx_ratio
    }).unwrap_or(true);

    // PHASE 6: Bonding Curve
    let phase6_curve = bonding_curve_from_features(&features);
    let phase6_passed = phase6_curve.map(|c| {
        c.bonding_progress_pct >= config.min_bonding_progress_pct
            && c.bonding_progress_pct <= config.max_bonding_progress_pct
            && c.current_market_cap_sol >= config.min_market_cap_sol
    }).unwrap_or(false);

    // ✅ ALL 6 PHASES COMPUTED SIMULTANEOUSLY, then combined
}
```

---

## 3. CONFIG THRESHOLDS - CO SIĘ FAKTYCZNIE UWAŻA

### Phase 1: Quantity Gate (HARD REQUIREMENT)
```toml
min_tx_count = 9
min_unique_signers = 8
min_buy_count = 7
```
**Logika:** `phase1_passed = (TX >= 9 AND signers >= 8 AND buys >= 7)`  
**Jeśli FALSE:** → REJECT natychmiast

### Phase 2: Velocity (SOFT)
```toml
min_avg_interval_ms = 1.0
max_avg_interval_ms = 400.0  # ⭐ p=1.2e-90 survival signal
min_timing_entropy = 0.01
max_burst_ratio = 1.0
```
**Logika:** `phase2_passed = (avg_interval IN [1, 400]) AND (entropy >= 0.01) AND (burst <= 1.0)`

### Phase 3: Diversity (SOFT)
```toml
max_hhi = 0.155
min_volume_gini = 0.56
```
**Logika:** `phase3_passed = diversity_phase_passes(diversity, config)`

### Phase 4: Volume (SOFT)
```toml
min_buy_ratio = 0.84
max_buy_ratio = 1.0
min_total_volume_sol = 1.0
```
**Logika:** `phase4_passed = volume_phase_passes(volume, config)`

### Phase 5: Dev Behavior (SOFT, default PASS)
```toml
max_dev_volume_ratio = 0.99
max_dev_tx_ratio = 0.99
reject_on_dev_sell = false
```
**Logika:** Jeśli dev_wallet unknown → PASS by default

### Phase 6: Bonding Curve (SOFT)
```toml
min_bonding_progress_pct = 48.0
max_bonding_progress_pct = 99.0
min_market_cap_sol = 50.0
```
**Logika:** `phase6_passed = (progress >= 48% AND progress <= 99% AND mcap >= 50 SOL)`

---

## 4. DECISION VERDICTS - TRZY WARSTWY ODRZUCEŃ

### Layer 1: Hard Filters (gatekeeper_policy.rs:608)
```rust
if let Some((_reason, reason_chain)) = evaluate_hard_filters_from_assessment(assessment, config) {
    return GatekeeperDecision {
        verdict_type: GatekeeperVerdictType::RejectHardFail,
        verdict_buy: false,
    };
}
```
**Hard fail warunki:**
- Phase 1 = false (za mało TX/signers/buys)
- Inne ekstremalne anomalie

### Layer 2: Core Phase Check (linie 633-647)
```rust
if !diagnostics.core1_passed || !diagnostics.core2_passed || !diagnostics.core3_passed {
    return GatekeeperDecision {
        verdict_type: GatekeeperVerdictType::RejectCoreFail,
        verdict_buy: false,
    };
}
```
Gdzie `core1 = phase1` (HARD), `core2 = phase4` (Volume)

### Layer 3: Soft Scoring (linie 675-687)
```rust
let total_soft_points = diagnostics.soft_points + diagnostics.sybil_policy.soft_points;
if diagnostics.soft_points > diagnostics.effective_max_soft_points {
    return GatekeeperDecision {
        verdict_type: GatekeeperVerdictType::RejectSoftExcess,
        verdict_buy: false,
    };
}
```

---

## 5. EXIT BEHAVIOR - RZECZYWISTA IMPLEMENTACJA

### Konfiguracja (shadow-burnin.toml, linie 54-55):
```toml
[trigger]
live_exit_take_profit_pct = 0.12  # 12% ✅ USED
live_exit_stop_loss_pct = 0.12    # -12% ✅ USED
```

### Config Flow (Proven):
```
main.rs (1845-1846):
  live_exit_take_profit_pct: config.trigger.live_exit_take_profit_pct,  // CZYTA Z CONFIG
  live_exit_stop_loss_pct: config.trigger.live_exit_stop_loss_pct,
        ↓
post_buy_runtime::PostBuyRuntimeConfig receives 0.12 values
        ↓
post_buy_runtime.rs (1732, 680, 639):
  fn live_exit_take_profit_bps(&self) -> u16 {
      percent_fraction_to_bps(self.live_exit_take_profit_pct)  // 0.12 → 1200 bps
  }
        ↓
Exit Price Calculation: entry_price * (1 + 0.12) = +12% trigger
```

### Exit Logic (post_buy_runtime.rs):
```rust
// Line 1730-1740 (simplified):
if current_price >= entry_price * (1.0 + config.live_exit_take_profit_pct) {
    trigger LiveExitTrigger::TakeProfit  // At +12%
}

if current_price <= entry_price * (1.0 - config.live_exit_stop_loss_pct) {
    trigger LiveExitTrigger::StopLoss    // At -12%
}
```

### Max Hold Time (main.rs, linia 1837):
```rust
max_ticks_before_exit: 240,  // @ 500ms tick = 120 sekund ✅ HARDCODED
```

### Config [exit_strategy] z ghost_brain_config.toml (linie 988-1017):
```toml
[exit_strategy]
tp_phase1_target_pct = 12.0      # ❌ ORPHANED (nie czytane z kodu)
tp_phase2_target_pct = 20.0      # ❌ ORPHANED
tp_phase3_target_pct = 35.0      # ❌ ORPHANED
tp_phase4_target_pct = 50.0      # ❌ ORPHANED
stop_loss_pct = -12.0             # ❌ ORPHANED
max_position_duration_ms = 1800000  # ❌ ORPHANED
```

**WNIOSEK:** 
- shadow-burnin.toml `[trigger]` sekcja IS USED (12% / -12%)
- ghost_brain_config.toml `[exit_strategy]` jest DEAD CODE (nigdy nie deserializowana)
- TP Ladder (phases 1-4) nie istnieje - to FLAT +12% lub -12%
- NO multi-phase exits
- NO soft stop @ 45s
- NO 30min max hold (tylko 120s hardcoded)

---

## 6. SYBIL RESISTANCE LAYER (FACTYCZNIE IMPLEMENTOWANY)

### Config (linie 167-191):
```toml
enable_sybil_interference_layer = true
max_sybil_soft_points = 6
min_fee_topology_diversity_index = 0.06
max_dev_buyer_infrastructure_affinity = 0.60
```

### Kod (gatekeeper_policy.rs:1176+):
```rust
pub fn compute_sybil_soft_signals(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV2Config,
) -> SybilAssessment {
    // DES (Duplicate Entry Syndrome)
    // SFD (Signer Fee Distribution)
    // FTDI (Fee Timing Divergence Index)
    // DBIA (DevBuy Infrastructure Affinity)
    // CPV (Cross-Pool Velocity)
    // FSC (Fee Structure Consistency) - FROZEN (penalty=0)
}
```

---

## 7. CO SIĘ NIE ZNALAZŁO W KODZIE

❌ **Phase 0** - Nie istnieje. Tylko Phase 1-6.

❌ **ab_window_ms jako decision trigger** - ab_window to 10000ms okno zbierania TX, NIE trigger decyzji.

❌ **Entry Verification @ T+6000ms** - Brak parametru, brak logiki. Config nie ma tego.

❌ **TP Ladder Executor** - Config istnieje (tp_phase1-4), ale kod je IGNORUJE.

❌ **Soft Stop Implementation** - Config ma `tp_phase1_soft_stop_hold_ms=45000` ale to się nie wykonuje.

---

## 8. RZECZYWISTY FLOW T0 → T+120s

```
T0: Pool created
│
T0-T10000ms: ab_window - zbieranie TX, akumulacja metryk
│            (oracle_runtime.rs:2493)
│
T0-T8000ms: phase collection window (max_wait_time_ms)
│           (gatekeeper_policy.rs)
│
T8000ms: DEADLINE
│        ├─ build_assessment_from_features() called
│        ├─ All 6 phases evaluated SIMULTANEOUSLY
│        ├─ Layer 1-3 verdicts computed
│        └─ GatekeeperDecision: Accept or Reject
│
IF Accept:
├─ Order bundle created + submitted
├─ TX lands on-chain
└─ Position opened
│
T8000+latency:
├─ post_buy_runtime starts monitoring
├─ Every tick (500ms):
│  ├─ Check current_price vs entry_price
│  ├─ If price >= entry * 1.02 → EXIT (take profit +2%)
│  ├─ If price <= entry * 0.98 → EXIT (stop loss -2%)
│  └─ Max 240 ticks allowed
│
T8000+120000ms:
└─ FORCE EXIT (max hold = 120s)

[Config values tp_phase1_target_pct=12.0, tp_phase2=20, etc. are NOT executed]
```

---

## 9. PODSUMOWANIE KODU vs SPEKULACJI

### ✅ PROVEN Z KODU:
- Phase 1-6 ewaluacja na deadline (8000ms)
- mode = "long" (całe okno, brak wczesnych decyzji)
- ab_window_ms = 10000ms (okno zbierania danych)
- 3-layer decision system (hard, core, soft)
- Sybil resistance layer (faktycznie w kodzie)
- **Take Profit = 12% z shadow-burnin.toml** ✅
- **Stop Loss = -12% z shadow-burnin.toml** ✅
- **Exit logic w post_buy_runtime.rs (linie 1730+)** ✅
- **Config read w main.rs (1845-1846)** ✅

### ❌ NIE ZNALEZIONO W KODZIE:
- Phase 0
- Entry verification @ T+6000ms
- TP Ladder phases 1-4 (config orphaned w ghost_brain_config.toml)
- Soft stop @ 45s (config orphaned)
- max_position_duration_ms = 1800s (config orphaned, kod: 120s)

### ⚠️  ORPHANED CONFIGS (dead-code):
- Cała sekcja `[exit_strategy]` w ghost_brain_config.toml (struktura ExitStrategyConfig nie istnieje w Rust)
- To jest planned-only/todo, nie implementowane

### 🟢 RZECZYWISTA BEHAWIOR (shadow-burnin.toml):
- Take profit: **+12%** (z [trigger])
- Stop loss: **-12%** (z [trigger])
- Max hold: **120 sekund** (hardcoded 240 ticks @ 500ms)
- Exit @ whichever hits first: TP or SL or timeout

---

## 10. PYTANIA DO ROZWIĄZANIA

1. **Dlaczego exit_strategy config istnieje ale nie jest używany?**
   - Czy to niedokończona implementacja?
   - Czy TP ladder ma być dodany?

2. **Jakie 2% (hardcoded) vs -2% (hardcoded) exit policy jest intentionalne?**
   - To jest asymetryczne do +12% target w config

3. **Dlaczego ab_window = 10000ms ale max_wait_time = 8000ms?**
   - 2000ms danych się gubi? Czy to bug?

4. **Phase 2-6 są soft czy hard?**
   - Kod traktuje je jako soft (tylko phase 1 + phase 4 to core)
   - Jeśli phase 2-6 są importantes, powinny być core?

---

**Koniec audytu. Tylko to co jest w kodzie.**
