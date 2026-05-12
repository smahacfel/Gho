# EXECUTIVE AUDIT SUMMARY — Gatekeeper V2/V2.5 Decision Pipeline
## Root Cause Analysis: BŁĘDY LOGIKI I KODU

> **Data:** 2026-05-07 | **Tryb:** mode=long, V2.5 shadow-first | **Wyłączone z analizy:** IWIM

---

## KRYTYCZNE BŁĘDY LOGIKI (P0)

### BUG-1 [P0]: Trzy niezależne implementacje hard faili z różnymi progami

**Opis:** System ma TRZY oddzielne implementacje tej samej logiki hard fail, każda z INNYMI progami:

| Hard fail | `run_assessment` (inline) | `compute_decision` | `evaluate_hard_filters_from_assessment` |
|-----------|--------------------------|-------------------|----------------------------------------|
| Bot timing | `interval_cv < 0.08 && avg < 30.0` **bez guarda min_tx** | `interval_cv < 0.08 && avg < 30.0` **z guardem** `hard_fail_bot_min_tx` | `interval_cv < 0.08 && avg < 30.0` **z guardem** `hard_fail_bot_min_tx` |
| Extreme HHI | **hardcoded `0.5`** | config `hard_fail_hhi`=`0.10` | config `hard_fail_hhi`=`0.10` |
| Price manipulation | **hardcoded `50.0%`** | config `max_single_tx_price_impact_pct` | config `max_single_tx_price_impact_pct` |
| Slow pool | **BRAK** | config `max_avg_interval_ms` | config `max_avg_interval_ms` |
| Market cap | **BRAK** | config `min_market_cap_sol` | config `min_market_cap_sol` |

**Lokalizacja:**
- `gatekeeper.rs:4860-4868` — bot detection BEZ guarda min_tx
- `gatekeeper.rs:4889` — hardcoded `hhi > 0.5` zamiast `config.hard_fail_hhi`
- `gatekeeper.rs:4977-4985` — hardcoded `impact > 50.0` zamiast config
- `gatekeeper.rs:4165-4327` — compute_decision HF-1 do HF-11 (config)
- `gatekeeper_policy.rs:696-849` — evaluate_hard_filters_from_assessment (config)

**Wpływ:** Pool z HHI=0.15 przy `hard_fail_hhi=0.10` przechodzi `run_assessment` (bo 0.15 < 0.5) ale zostaje odrzucony przez `compute_decision` (bo 0.15 > 0.10). **Werdykt zależy od ścieżki kodu, nie od stanu poolu.**

**Dowód z kodu:**
```rust
// run_assessment:4889 — WARTOŚĆ NA SZTYWNO 0.5
if hard_reject_reason.is_none() && diversity.hhi > 0.5 {

// compute_decision:4243-4252 — Z KONFIGU 0.10  
if div.hhi > cfg.hard_fail_hhi {  // cfg.hard_fail_hhi = 0.10

// policy.rs:764 — Z KONFIGU 0.10
if diversity.hhi > config.hard_fail_hhi {  // config.hard_fail_hhi = 0.10
```

**Dlaczego to jest katastrofalne:** `evaluate_phases` (legacy path) sprawdza `assessment.hard_reject_reason` ustawione przez `run_assessment`. Pool z HHI=0.15 przechodzi legacy path (bo 0.15 < 0.5) ale zostałby odrzucony przez 3-layer path (bo 0.15 > 0.10). **Ten sam pool, ten sam stan, różne werdykty w zależności od tego, czy `use_three_layer_decision` jest włączone.**

---

### BUG-2 [P0]: Trajektoria (TAS) jest MARTWYM KODEM w ścieżce LIVE

**Opis:** `build_assessment_from_features` (policy.rs) **NIGDY** nie ustawia pola `trajectory`. Zawsze jest `None`:

```rust
// gatekeeper_policy.rs:497-498
trajectory_available: false,
trajectory: None,
```

Trajektoria jest obliczana tylko w `run_assessment` (buffer path) przez `self.materialize_trajectory()`:

```rust
// gatekeeper.rs:5021 (w run_assessment)
let trajectory = self.materialize_trajectory(&self.config.tas);
```

**Ścieżka LIVE** (Long mode → `evaluate_feature_driven_terminal_verdict` → `evaluate_from_features` → `build_assessment_from_features`) nigdy nie przechodzi przez `run_assessment`. Używa `build_assessment_from_features` z policy.rs, gdzie `trajectory = None`.

**Lokalizacja:**
- `gatekeeper_policy.rs:497-498` — trajectory na sztywno None
- `gatekeeper.rs:5021` — trajectory tylko w run_assessment (buffer path)
- `oracle_runtime.rs:4794-4797` — evaluate_from_features używa build_assessment_from_features

**Wpływ:** `evaluate_policy_from_assessment` (policy.rs:1084) próbuje odczytać `assessment.trajectory`:
```rust
let tas_score = if config.tas.enabled {
    assessment.trajectory.as_ref().map(|t| t.overall_tas_score)
} else { None };
```
W ścieżce LIVE, `assessment.trajectory` jest ZAWSZE None → `tas_score` jest ZAWSZE None → TAS NIGDY nie moduluje confidence ani nie demotuje Strong→Borderline w LIVE path. **Cały moduł TAS jest martwy w produkcji.**

---

### BUG-3 [P0]: `run_assessment` nie sprawdza slow pool (avg_interval_ms)

**Opis:** `run_assessment` (buffer path) sprawdza bot timing (interval_cv + avg_interval) ale NIGDY nie sprawdza `avg_interval_ms > max_avg_interval_ms`. Ten check istnieje tylko w `compute_decision` (HF-11) i `evaluate_hard_filters_from_assessment`.

**Lokalizacja:**
- `gatekeeper.rs:4859-4868` — tylko bot detection, brak slow pool
- `gatekeeper.rs:4318-4327` — HF-11 w compute_decision
- `gatekeeper_policy.rs:813-822` — slow pool w policy

**Wpływ:** W legacy path (`evaluate_phases` bez 3-layer), slow pool NIGDY nie zostanie odrzucony na poziomie `run_assessment`. W 3-layer path — zostanie odrzucony przez `compute_decision`. Kolejna niespójność między ścieżkami.

---

### BUG-4 [P0]: `materialize_pdd_diagnostics_from_features` NIGDY nie wykrywa spike/ramping/flash crash

**Opis:** Feature-driven PDD (policy.rs:598-656) używa TYLKO `MaterializedFeatureSet` i sprawdza wyłącznie:
- entry_drift (z checkpoint/account features)
- whale top3 (z tx_intel_features.top3_volume_pct)
- reserve health (z account_features)

**Nie sprawdza:** spike, ramping, flash crash — bo te wymagają sekwencji transakcji (bufora).

Pełny `evaluate_pdd` (pdd.rs:80) sprawdza wszystkie 6 sygnałów, ale jest dostępny tylko przez bufor (`run_assessment` → `evaluate_pdd`).

**Lokalizacja:**
- `gatekeeper_policy.rs:598-656` — feature-driven PDD (3/6 sygnałów)
- `gatekeeper_pdd.rs:80-188` — pełny PDD (6/6 sygnałów)
- `AUDYT_PIPELINE_GATEKEEPER_V2.md:195-208` — udokumentowane ryzyko

**Wpływ:** Gdy `live_execution_enabled = true` i PDD promowane do live, ścieżka LIVE używa feature-driven PDD który NIE WYKRYJE spike/ramping/flash. Pool z wykrytym rampingiem w shadow dostanie BUY w live. **System podejmie decyzję kupna na poolu, który shadow oznaczył jako pump-and-dump.**

**Dowód:** W `materialize_pdd_diagnostics_from_features` nie ma ani jednego wywołania `detect_spike`, `detect_ramping`, `detect_flash_crash`. Te funkcje istnieją tylko w `gatekeeper_pdd.rs` i są wywoływane wyłącznie z `evaluate_pdd`.

---

## BŁĘDY WYSOKIEGO PRIORYTETU (P1)

### BUG-5 [P1]: Przekłamanie `whale_top3_pct` między feature-driven a buffer PDD

**Opis:** Feature-driven PDD liczy whale_top3_pct z `features.tx_intel_features.top3_volume_pct * 100.0`. Buffer PDD liczy z per-signer volumes z bufora (`detect_whale_concentration` w pdd.rs:350-369). Te dwie wartości mogą być różne, bo:
- `tx_intel_features.top3_volume_pct` może być obliczone w innym momencie
- Buffer ma pełną historię transakcji, features mają snapshot

**Lokalizacja:**
- `gatekeeper_policy.rs:631` — `features.tx_intel_features.top3_volume_pct * 100.0`
- `gatekeeper_pdd.rs:350-369` — liczone z `buffer.signer_stats()`

**Wpływ:** Shadow i live mogą mieć różne wartości whale detection dla tego samego poolu.

---

### BUG-6 [P1]: `v25_confidence()` używa `decision.alpha_gate` który może być `not_run()`

**Opis:** `v25_confidence()` (gatekeeper.rs:1309-1358) pobiera alpha_gate diagnostics z `self.decision.as_ref()?.alpha_gate`. Jeśli decyzja powstała z hard fail lub core fail, `alpha_gate` jest ustawione na `AlphaGateDiagnostics::not_run()` — gdzie `momentum`, `demand`, `joint` są `None`. Wtedy `alpha_quality` = 0.0:

```rust
let momentum = decision.alpha_gate.momentum.unwrap_or(0.0); // = 0.0
let demand = decision.alpha_gate.demand.unwrap_or(0.0);       // = 0.0
let joint = momentum * demand;                                  // = 0.0
let alpha_quality = (0.0 * 0.4 + 0.0 * 0.35 + 0.0 * 0.25).clamp(0.0, 1.0); // = 0.0
```

Wynik: `confidence = base_quality * 0.0 * pdd_modulator * tas_modulator * sybil_modulator = 0.0`.

To oznacza, że **KAŻDY pool który przeszedł przez hard fail lub core fail ścieżkę, a potem został z jakiegoś powodu sprawdzony przez `v25_confidence()`, dostanie confidence = 0.0**. Ale to jest OK, bo taki pool nie powinien być BUY. Problem pojawia się gdy `alpha_gate` zostało pominięte z innego powodu (np. `insufficient_sample`), bo wtedy też daje 0.0.

**Lokalizacja:** `gatekeeper.rs:1312` — `let decision = self.decision.as_ref()?;` + `gatekeeper.rs:1319-1322`

**Wpływ:** Średni. Pool z `insufficient_sample` dla alpha gate dostaje `alpha_quality = 0.0` → confidence = 0.0. To zbyt restrykcyjne — brak próbki nie powinien oznaczać zerowej confidence.

---

### BUG-7 [P1]: APS `evaluate_aps()` dostaje `spike_detected = false` w ścieżce feature-driven

**Opis:** W `build_assessment_from_features` (policy.rs:520-528), APS jest wywoływane z `pdd.spike_detected` z feature-driven PDD. Ale feature-driven PDD NIGDY nie ustawia `spike_detected = true`, bo nie ma detekcji spike. Więc APS zawsze dostaje `false` w ścieżce feature-driven.

W `try_shadow_evaluate` (gatekeeper.rs:5256-5262), APS dostaje poprawną wartość z buffer PDD.

**Lokalizacja:**
- `gatekeeper_policy.rs:520-528` — APS z feature-driven PDD (spike zawsze false)
- `gatekeeper.rs:5256-5262` — APS z buffer PDD (spike poprawny)

**Wpływ:** APS w live path nigdy nie widzi PDD spike, więc `detect_regime` ma mniej sygnałów do wykrycia HighVolatility.

---

### BUG-8 [P1]: `try_shadow_evaluate` używa uproszczonego confidence zamiast modelu V2.5

**Opis:** Shadow confidence w `try_shadow_evaluate` (gatekeeper.rs:5299-5304):
```rust
let confidence = if decision.max_soft_points_possible > 0 {
    let ratio = decision.soft_points as f64 / decision.max_soft_points_possible as f64;
    (1.0 - ratio).clamp(0.0, 1.0)
} else { 0.0 };
```

Pełny model V2.5 `v25_confidence()` (gatekeeper.rs:1309-1358):
```rust
base_quality * alpha_quality * pdd_modulator * tas_modulator * sybil_modulator
```

Te dwie formuły dają fundamentalnie różne wyniki. Shadow confidence ignoruje alpha gate, PDD score, i TAS modulation w bazowej kalkulacji (dodaje je później jako mnożniki). Pełny model V2.5 używa podejścia multiplikatywnego od początku.

**Wpływ:** Shadow checkpointy pokazują inną confidence niż terminalny werdykt. Trudność w kalibracji i porównywaniu.

---

### BUG-9 [P1]: `run_assessment` na linii 4889 używa `hhi > 0.5` — 5x wyższy niż config

**Opis:** Hardcoded `0.5` w `run_assessment` vs config `hard_fail_hhi = 0.10`. Różnica 5x. `run_assessment` odrzuca tylko ekstremalnie skoncentrowane pule (HHI > 0.5), podczas gdy `compute_decision` odrzuca już przy HHI > 0.10.

**Lokalizacja:** `gatekeeper.rs:4889`

**Wpływ:** W legacy path, pule z HHI 0.11-0.49 przechodzą przez `run_assessment` bez hard faila, ale zostałyby odrzucone przez `compute_decision`.

---

## BŁĘDY ŚREDNIEGO PRIORYTETU (P2)

### BUG-10 [P2]: `has_sufficient_history = false` na sztywno — APS zawsze zwraca Normal

**Opis:** W `gatekeeper_adaptive_prosperity.rs:97`, `has_sufficient_history` jest hardcodowane na `false`. Oznacza to, że wykrywanie reżimu (HighVolatility/LowVolatility) nigdy nie jest aktywne, mimo że funkcja `detect_regime()` istnieje i działa poprawnie.

**Lokalizacja:** `gatekeeper_adaptive_prosperity.rs:97`

**Wpływ:** System nie adaptuje progów do warunków rynkowych. Kod adaptacji istnieje, ale jest martwy.

---

### BUG-11 [P2]: `check_long_deadline` ignoruje `hard_reject_reason` z `run_assessment` przy 3-layer

**Opis:** `check_long_deadline` (gatekeeper.rs:5911-5923):
1. Woła `run_assessment()` → ustawia `assessment.hard_reject_reason` (inline hard fail z hardcoded progami)
2. Woła `compute_decision(&assessment)` → tworzy NOWY hard_fail_reason (z config progami)
3. NIGDY nie sprawdza `assessment.hard_reject_reason` przed podjęciem decyzji

`hard_reject_reason` z `run_assessment` jest całkowicie ignorowane, gdy 3-layer decision jest włączone.

**Lokalizacja:** `gatekeeper.rs:5911-5923`

**Wpływ:** Niski w produkcji (3-layer ON), ale `run_assessment` wykonuje niepotrzebną pracę obliczając hard rejeckty, które są potem ignorowane.

---

### BUG-12 [P2]: `build_assessment_from_features` nadpisuje `hard_reject_reason` z decyzji

**Opis:** W `evaluate_from_features` (gatekeeper.rs:3237-3241):
```rust
assessment.hard_reject_reason = assessment
    .decision
    .as_ref()
    .and_then(|decision| decision.hard_fail_reason.clone());
```

To nadpisuje `hard_reject_reason`, który został ustawiony przez `evaluate_hard_filters_from_assessment` wewnątrz `build_assessment_from_features` (policy.rs:533-534). Wartość z policy jest tracona.

**Lokalizacja:** `gatekeeper.rs:3237-3241`

**Wpływ:** Niski — oba źródła powinny dać ten sam wynik (używają tego samego configu), ale jeśli się różnią, wartość z `decision.hard_fail_reason` wygrywa.

---

### BUG-13 [P2]: `evaluate_phases` ma ścieżkę legacy która używa `hard_reject_reason` bezpośrednio

**Opis:** `evaluate_phases` (gatekeeper.rs:4704-4717):
```rust
// Legacy Decision System (phases_passed)
if assessment.hard_reject_reason.is_some() {
    self.rejected = true;
    let reason = assessment.hard_reject_reason.clone().unwrap();
    return GatekeeperVerdict::Reject { assessment, reason };
}
```

W legacy path, hard rejeckty z `run_assessment` (hardcoded progi) są honorowane bezpośrednio. W 3-layer path — nie. To kolejny przykład niespójności między ścieżkami.

**Lokalizacja:** `gatekeeper.rs:4704-4717`

---

## REKOMENDACJE NAPRAWCZE

### P0 — Natychmiastowe

**FIX-BUG-1: Ujednolicić hard fail detection**
Usunąć hardcoded wartości z `run_assessment` (linie 4860-4868, 4889, 4977-4985) i zastąpić je wywołaniem wspólnej funkcji hard fail z configu, identycznej jak w `compute_decision` i `evaluate_hard_filters_from_assessment`.

```rust
// gatekeeper.rs:4859-4894 — ZAMIAST hardcoded checków:
// Usunąć inline hard reject checks z run_assessment.
// compute_decision i tak je sprawdza (w 3-layer path).
// W legacy path — dodać wywołanie evaluate_hard_filters_from_assessment.
```

**FIX-BUG-2: Dodać trajectory do `build_assessment_from_features`**

```rust
// gatekeeper_policy.rs:497-498 — ZAMIAST:
trajectory_available: false,
trajectory: None,
// POTRZEBNA: Możliwość przekazania trajectory do build_assessment_from_features
// przez rozszerzenie MaterializedFeatureSet o dane segmentów,
// LUB przez nowy parametr funkcji.
```

Alternatywnie: przenieść decyzję LIVE na `run_assessment` + `compute_decision` zamiast `build_assessment_from_features` + `evaluate_policy_from_assessment`.

**FIX-BUG-4: Rozszerzyć `materialize_pdd_diagnostics_from_features` o pełne PDD**

Wymaga rozszerzenia `MaterializedFeatureSet` o dane sekwencji transakcji (checkpoint_features z segmentami czasowymi) lub przekazania bufora do funkcji feature-driven.

### P1 — Wysoki priorytet

**FIX-BUG-8: Użyć `v25_confidence()` w `try_shadow_evaluate`**

```rust
// gatekeeper.rs:5299-5304 — ZAMIAST uproszczonego:
let confidence = assessment.v25_confidence(&self.config).unwrap_or(0.0);
// UWAGA: wymaga, żeby assessment.decision było ustawione przed tą linią.
```

**FIX-BUG-9: Wyrównać hardcoded HHI w `run_assessment`**

```rust
// gatekeeper.rs:4889 — ZAMIAST:
if hard_reject_reason.is_none() && diversity.hhi > 0.5 {
// NA:
if hard_reject_reason.is_none() && diversity.hhi > self.config.hard_fail_hhi {
```

---

## CHECKLIST KOMPLETNOŚCI

- [x] `AUDYT_PIPELINE_GATEKEEPER_V2.md` — wczytany i przeanalizowany
- [x] `gatekeeper.rs` (11,879 linii) — przeanalizowany: `run_assessment`, `compute_decision`, `check_long_deadline`, `try_shadow_evaluate`, `evaluate_from_features`, `evaluate_phases`, `materialize_trajectory`, `v25_confidence`, `v25_tas_hard_reject`
- [x] `gatekeeper_policy.rs` (2,473 linii) — przeanalizowany: `build_assessment_from_features`, `evaluate_policy_from_assessment`, `evaluate_hard_filters_from_assessment`, `materialize_pdd_diagnostics_from_features`, `evaluate_alpha_gate`, `evaluate_prosperity_filter`, `evaluate_curve_gate`, `compute_core3_pass`, `build_policy_diagnostics`
- [x] `gatekeeper_pdd.rs` (566 linii) — przeanalizowany: `evaluate_pdd`, 6 detection functions, 4-level anchor hierarchy
- [x] `gatekeeper_trajectory.rs` (207 linii) — przeanalizowany: `score_trajectory`, `build_segment`, `compute_tas_modulator`
- [x] `gatekeeper_adaptive_prosperity.rs` (397 linii) — przeanalizowany: `evaluate_aps`, `detect_regime`, `compute_shadow_prosperity_pass`
- [x] `gatekeeper_v25_config.rs` (427 linii) — przeanalizowane wszystkie structy, Default impls, serde
- [x] `oracle_runtime.rs` — przeanalizowane: `evaluate_feature_driven_terminal_verdict`, `resolve_feature_trigger_outcome`
- [x] `ghost_brain_config.toml` — pełna analiza konfiguracji deploy v11
- [x] `ghost_brain_config.rs` — analiza Default vs TOML rozbieżności
- [x] Flow trace: Long mode → `check_long_deadline` → `run_assessment` → `compute_decision`
- [x] Flow trace: Long mode → `evaluate_feature_driven_terminal_verdict` → `build_assessment_from_features` → `evaluate_policy_from_assessment`
- [x] Flow trace: Shadow checkpoints → `try_shadow_evaluate` → `run_assessment` → `compute_decision`
- [x] Identyfikacja 3 niezależnych implementacji hard fail
- [x] Identyfikacja martwego kodu TAS w ścieżce LIVE
- [x] Identyfikacja wszystkich hardcoded wartości w logice

---

## ODPOWIEDŹ NA KLUCZOWE PYTANIE

**CO JEST OBECNIE PROBLEMEM Z NASZYM SYSTEMEM DECYZYJNYM?**

System decyzyjny Gatekeeper V2 cierpi na **fundamentalną niespójność implementacyjną**: istnieją trzy oddzielne implementacje tej samej logiki hard fail, każda z innymi progami (hardcoded vs config). `run_assessment` używa `hhi > 0.5` podczas gdy `compute_decision` używa `hard_fail_hhi = 0.10` z configu. Oznacza to, że werdykt zależy od ścieżki kodu, którą przechodzi pool — nie od jego rzeczywistego stanu.

Drugi krytyczny problem: **trajektoria (TAS) jest martwym kodem w ścieżce LIVE**. `build_assessment_from_features` nigdy nie ustawia pola `trajectory`. Cała logika TAS — segmentacja 3-okienna, scoring momentum, HHI trajectory, volume consistency — działa tylko w shadow checkpointach. W produkcji (Long mode → feature-driven evaluation), TAS nigdy nie wpływa na decyzję.

Trzeci problem: **PDD feature-driven vs buffer split**. Feature-driven PDD sprawdza 3 z 6 sygnałów. Nie wykrywa spike, ramping, ani flash crash. Przy przełączeniu `live_execution_enabled = true`, system będzie podejmował decyzje BUY na poolach, które shadow oznacza jako pump-and-dump.

Te trzy problemy razem oznaczają, że system **nie jest deterministyczny między ścieżkami**, **ignoruje kluczowe sygnały w live path**, i **nie wykorzystuje połowy zaimplementowanej logiki V2.5 w produkcji**.
