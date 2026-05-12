# V2.5 Shadow Burn-in — Raport z monitoringu

**Data:** 2026-05-03 23:30
**Proces:** PID 301091, uruchomiony 23:00
**Branch:** `main` (commit `eaecac4` + uncommitted V2.5 changes)
**Konfiguracja launch:** `/home/ghost/Gho/configs/rollout/shadow-burnin.toml`
**Konfiguracja ghost-brain:** `/home/ghost/Gho/ghost-brain/ghost_brain_config.toml`

---

## 1. Status kompilacji i testów

| Element | Wynik |
|---------|-------|
| Binary | `/home/ghost/Gho/target/release/ghost-launcher` — zbudowany 23:05 |
| Schema JSONL | **v16** potwierdzone — `log_schema_version: 16`, `gatekeeper_version: "v2.5"` |
| Testy lib | **166/166 passed** (`cargo test -p ghost-launcher --lib -- "components::gatekeeper"`) |
| Testy integracyjne | **14/14 passed** (PDD 6, TAS 4, Regression 4) |
| Decision logger | **11/11 passed** |
| Workspace check | `cargo check -p ghost-launcher --tests` → **0 errors** |

---

## 2. Analiza decyzji — 20 minut runu

### 2.1 Rozkład werdyktów (schema v16, 910 decyzji)

```
TIMEOUT_PHASE1:          626  (68.8%)  ← Phase 1 nie przeszedł
REJECT_HARD_FAIL:        132  (14.5%)  ← Phase 1 OK, hard fail
TIMEOUT_NO_DATA:         123  (13.5%)  ← Brak danych w deadline
REJECT_CORE_FAIL:         23   (2.5%)  ← Core-2/Core-3 fail
REJECT_LOW_PROSPERITY:     2   (0.2%)  ← Prosperity filter odrzucił
REJECT_SYBIL_INTERFERENCE: 2   (0.2%)
REJECT_SYBIL_SOFT_EXCESS:  2   (0.2%)
BUY:                        0   (0.0%)  ← ŻADEN pool nie dostał BUY
```

### 2.2 Top przyczyny REJECT_HARD_FAIL (132 decyzje)

```
market_cap < 45.0 SOL       (dominujące)  ← większość pooli 28-44 SOL
price_change_ratio > 1.6                  ← cena już wystrzeliła
```

**Przykłady:**
- `HARD_FAIL: market_cap=28.7 < 45.0`
- `HARD_FAIL: market_cap=35.6 < 45.0`
- `HARD_FAIL: market_cap=44.2 < 45.0` (0.8 SOL od progu!)
- `HARD_FAIL: price_change_ratio=2.0 > 1.6`

### 2.3 Top przyczyny REJECT_CORE_FAIL (23 decyzje)

```
CORE_FAIL: core1=true core2=false core3=true  ← Core-2 (Capital Dominance / Phase 4)
```

### 2.4 ROOT CAUSE — dlaczego V2.5 shadow nie ma danych

**Problem nie leży w Phase 1.** 132 poole przeszły Phase 1 i dotarły do HARD_FAIL.

**Problem leży w warstwie Hard Fail:**
1. **`min_market_cap_sol = 45`** — odcina ~90% pooli na pump.fun. Większość ma market cap 28-44 SOL.
2. **`max_price_change_ratio = 1.6`** — cena już poszła >60% od początku, co triggeruje hard fail.

Poole które przechodzą Phase 1 → dostają HARD_FAIL zanim V2.5 zdąży zrobić shadow ewaluację.

**V2.5 shadow pipeline wymaga**, żeby pool przeżył do checkpointów czasowych (2-5s early, 7s normal, 8s terminal). Ale hard fail zabija pool NATYCHMIAST po osiągnięciu deadline'u.

---

## 3. Aktywne progi — PRIMARY DECISION FACTORS (potwierdzone)

| Warstwa | Próg | Rola w blokowaniu BUY |
|---------|------|----------------------|
| **Hard Fail: market_cap** | `min_market_cap_sol = 45` | **GŁÓWNY BLOKER** — zabija ~90% pooli |
| **Hard Fail: price_change** | `max_price_change_ratio = 1.6` | Drugi główny bloker |
| Phase 1 | TX ≥ 12, signers ≥ 8, buys ≥ 6 | 68.8% pooli nie przechodzi |
| Core-2 | Capital Dominance (Phase 4) | 2.5% pooli |
| Prosperity | mcap ≥ 45, branch matching | 0.2% pooli |
| V2.5 SHADOW | `live_execution_enabled = false` | **NIE BLOKUJE** — shadow-only |

---

## 4. V2.5 Shadow Pipeline — stan

### 4.1 Co jest gotowe
- Wszystkie 4 moduły (DOW, TAS, PDD, APS) skompilowane i przetestowane ✅
- Schema v16 w JSONL ✅
- Shadow triggery w `on_transaction_long()` ✅
- Extended shadow w `check_long_deadline()` ✅

### 4.2 Dlaczego shadow decisions = None
Hard fail blokuje pool ZANIM V2.5 shadow checkpointy (2-5s, 7s) zdążą odpalić. Żeby V2.5 shadow się pojawił, potrzebny jest pool który:
1. Przejdzie Phase 1 w ciągu 2-5s (≥12 TX, ≥8 signers, ≥6 buys)
2. NIE zostanie zabity przez hard fail (market_cap ≥ 45 SOL, price_change ≤ 1.6)
3. Dotrwa do shadow checkpointu

### 4.3 Sugestia — test V2.5 z łagodniejszymi progami
Aby zweryfikować V2.5 shadow pipeline na żywych danych, można obniżyć progi hard fail:
```toml
# W ghost_brain_config.toml:
min_market_cap_sol = 25   # zamiast 45
max_price_change_ratio = 3.0  # zamiast 1.6
```
To przepuści więcej pooli do V2.5 shadow evaluation bez zmiany live werdyktu (`live_execution_enabled` wciąż `false`).

---

## 5. Ścieżki do wszystkich artefaktów

| Artefakt | Ścieżka |
|----------|---------|
| Binary | `/home/ghost/Gho/target/release/ghost-launcher` |
| Config launcher | `/home/ghost/Gho/configs/rollout/shadow-burnin.toml` |
| Config ghost-brain (V2.5) | `/home/ghost/Gho/ghost-brain/ghost_brain_config.toml` |
| V2.5 config structs | `/home/ghost/Gho/ghost-brain/src/config/gatekeeper_v25_config.rs` |
| JSONL decisions (v16) | `/home/ghost/Gho/logs/decisions.json/rollout/shadow-burnin/decisions/gatekeeper_v2_decisions.jsonl` |
| JSONL BUYs | `/home/ghost/Gho/logs/decisions.json/rollout/shadow-burnin/decisions/gatekeeper_v2_buys.jsonl` |
| Gatekeeper V2.5 main | `/home/ghost/Gho/ghost-launcher/src/components/gatekeeper.rs` |
| PDD module | `/home/ghost/Gho/ghost-launcher/src/components/gatekeeper_pdd.rs` |
| TAS module | `/home/ghost/Gho/ghost-launcher/src/components/gatekeeper_trajectory.rs` |
| APS module | `/home/ghost/Gho/ghost-launcher/src/components/gatekeeper_adaptive_prosperity.rs` |
| Policy path (V2.5) | `/home/ghost/Gho/ghost-launcher/src/components/gatekeeper_policy.rs` |
| Decision logger (v16) | `/home/ghost/Gho/ghost-brain/src/oracle/decision_logger.rs` |
| PDD tests | `/home/ghost/Gho/ghost-launcher/tests/gatekeeper_pdd_tests.rs` |
| TAS tests | `/home/ghost/Gho/ghost-launcher/tests/gatekeeper_tas_tests.rs` |
| Regression tests | `/home/ghost/Gho/ghost-launcher/tests/gatekeeper_v25_regression.rs` |
| Plan V2.5 | `/home/ghost/Gho/PLANS/GATEKEEPER_V25_PRECISION_STRIKE.md` |
| SSOT contracts | `/home/ghost/Gho/PLANS/GATEKEEPER_V25_SSOT_CONTRACTS.md` |
| Ten raport | `/home/ghost/Gho/V25_SHADOW_BURNIN_MONITORING_REPORT.md` |
