# PLAN PRZEBUDOWY LOGIKI FILTROWANIA PULI
**Data:** 2026-04-01  
**Autor:** Ghost Father  
**Status:** DO EGZEKUCJI  
**Priorytet:** KRYTYCZNY

---

## 1. DIAGNOZA STANU — CO JEST NIE TAK I DLACZEGO

### Layer 3 (Soft Scoring) jest w farmakologicznej śpiączce

Gatekeeper ma 3-warstwowy system decyzyjny. Warstwa 3 (soft scoring) działała poprawnie architekturalnie, ale była wyłączona na **dwa jednoczesne sposoby**:

**Zamrożenie #1 — próg gilotyny:**
```toml
max_soft_points = 255  # ← nigdy nie zostanie przekroczone
```

**Zamrożenie #2 — progi flag ustawione na ∞:**
```toml
max_same_ms_tx_ratio = 1.0   # bundle_suspicion nigdy nie odpala
max_hhi = 9.0                # cabal_suspicion nigdy nie odpala
max_top3_volume_pct = 1.0    # top3_dominance nigdy nie odpala
```

Efekt: Layer 3 nie odrzucił **żadnej** puli od momentu zamrożenia.

### Dane są, pipeline jest. Brakuje tylko klucza do stacyjki.

Audit `gatekeeper_v2_buys.jsonl` (4488 rekordów BUY z aktywnego runu) pokazuje:

| Metryka | Pokrycie danych | % BUY-ów powyżej progu A/B |
|---|---|---|
| `block0_sniped_supply_pct` | **100% (0% null)** | **50.9%** (próg: ≥0.117009) |
| `early_slot_volume_dominance_buy` | **100% (0% null)** | **76.7%** (próg: ≥0.2743) |
| `same_ms_tx_ratio` | **100% (0% null)** | **51.8%** (próg: ≥0.233) |
| `dev_paperhand_latency_ms` | **29.5% (70.5% null)** | 32.8% z populowanych (<2575ms) |

**Interpretacja krytyczna:** Połowa do trzech czwartych obecnych BUY-ów niesie sygnały B-like, które nigdy nie były oceniane. Dane są w pełni dostępne w runtime. System jest ślepy z założenia, nie z braku danych.

---

## 2. ARCHITEKTURA ZMIANY — CO RUSZAMY, CZEGO NIE RUSZAMY

### Co NIE ulega zmianie:
- żadnych nowych typów, modułów, klas
- żadnych zmian w `oracle_runtime.rs`
- żadnych zmian w `MaterializedFeatureSet`
- żadnych zmian w `gatekeeper_policy.rs`
- refaktor PR3-PR8 poczeka

### Co ulega zmianie:
1. **`ghost_brain_config.toml`** — odmrożenie progów i wag (zero kodu)
2. **`ghost-launcher/src/components/gatekeeper.rs`** — `compute_soft_signals()`: ~20 linii Rust wzbogacających istniejące flagi o dane z `early_fingerprint`

### Gdzie NIE jest `compute_soft_signals`:
Funkcja jest w `ghost-launcher/src/components/gatekeeper.rs` (~linia 2812), **NIE** w `gatekeeper_policy.rs`.

---

## 3. KROK 1 — ODMROŻENIE CONFIGU (czas: ~10 minut, zero ryzyka regresji)

Plik: `ghost-brain/ghost_brain_config.toml`, sekcja `[gatekeeper_v2]`

### Zmiany w sekcji Phase 3 (aktywacja istniejących flag):
```toml
# Było:
max_hhi = 9.0
max_top3_volume_pct = 1.0
max_same_ms_tx_ratio = 1.0

# Staje się:
max_hhi = 0.056               # cabal_suspicion aktywny (z raportu A/B)
max_top3_volume_pct = 0.27    # top3_dominance aktywny
max_same_ms_tx_ratio = 0.233  # bundle_suspicion aktywny (Youden J z raportu)
```

### Zmiany wag soft scoringu:
```toml
# Było:
soft_weight_timing = 1
soft_weight_manipulation = 3
soft_weight_diversity = 2
soft_weight_ecosystem = 1
max_soft_points = 255           # ←  ZAMROŻONE
dev_unknown_max_soft_points = 255

# Staje się:
soft_weight_timing = 15         # Conf: dev_paperhand ~0.152, ab_tx ~0.165
soft_weight_manipulation = 23   # Conf: snajperzy ~0.228, early_slot ~0.219
soft_weight_diversity = 16      # Conf: same_ms ~0.117, fixed_size ~0.143
soft_weight_ecosystem = 14      # Conf: price_change ~0.141
max_soft_points = 70            # GILOTYNA OPUSZCZONA — próg kalibracyjny
dev_unknown_max_soft_points = 50
```

### Logika progu 70:
- 1 flaga Manipulation = 23 pkt → BUY
- 2 flagi Manipulation = 46 pkt → BUY
- 3 flagi Manipulation (bundle + cabal + top3) = 69 pkt → BUY z marginesem 1
- 3 flagi Manipulation + 1 flaga Timing = 69+15 = 84 pkt → **REJECT**
- Czysta patologia (5+ flag) → **REJECT** zdecydowanie

Próg 70 jest **celowo restrykcyjny** na start. Po walidacji w shadow mode można go podnieść jeśli dobry volume spada.

### Hard Fails — ZOSTAWIAMY BEZ ZMIAN:
`reject_on_dev_sell = false` — pozostaje. `dev_has_sold` nie wraca do Hard Fails dopóki `dev_paperhand_latency_ms` nie ma pokrycia >80%.

---

## 4. KROK 2 — WERYFIKACJA PRZED WDROŻENIEM KROKU 3

Po wdrożeniu Kroku 1 uruchom shadow run przez min. 2h i sprawdź:

### Narzędzie diagnostyczne (właściwy plik!):
```bash
# Właściwy plik to gatekeeper_v2_decisions.jsonl / gatekeeper_v2_buys.jsonl
# NIE buys.jsonl z shadow_run/ (to jest plik wyników egzekucji, nie decyzji GK)

GKFILE="logs/decisions.jsonl/gatekeeper_v2_decisions.jsonl"

# Ile decyzji REJECT_SOFT_EXCESS po odmrożeniu?
jq -r 'select(.verdict_type == "REJECT_SOFT_EXCESS") | .decision_reason' "$GKFILE" | head -20

# Rozkład soft_points wśród BUY-ów:
jq -r 'select(.decision_verdict_buy == true) | .soft_points // 0' "$GKFILE" | \
  awk '{b[int($1/10)*10]++} END {for(k in b) print k"-"k+9": "b[k]}' | sort -n

# Jaki % BUY-ów miałby REJECT gdyby próg wynosił X:
for threshold in 46 60 70 80; do
  count=$(jq -r "select(.decision_verdict_buy == true) | .soft_points // 0" "$GKFILE" | awk -v t=$threshold '$1>t{c++}END{print c+0}')
  total=$(jq -r 'select(.decision_verdict_buy == true) | 1' "$GKFILE" | wc -l)
  echo "threshold=$threshold: rejects=$count/$total ($(echo "scale=1; $count*100/$total" | bc)%)"
done
```

### Null-rate check nowych flag fingerprinta:
```bash
GKFILE="logs/decisions.jsonl/gatekeeper_v2_buys.jsonl"
echo "=== block0_sniped_supply_pct ===" && \
  jq -r 'if .block0_sniped_supply_pct == null then "null" else "value" end' "$GKFILE" | sort | uniq -c
echo "=== early_slot_volume_dominance_buy ===" && \
  jq -r 'if .early_slot_volume_dominance_buy == null then "null" else "value" end' "$GKFILE" | sort | uniq -c
echo "=== dev_paperhand_latency_ms ===" && \
  jq -r 'if .dev_paperhand_latency_ms == null then "null" else "value" end' "$GKFILE" | sort | uniq -c
```

**Oczekiwany wynik:** block0 i early_slot = 0% null (potwierdzono na 4488 rekordach).  
**Jeśli null > 20%** na nowych danych = problem z parserem lub brak gRPC — wstrzymać Krok 3.

---

## 5. KROK 3 — WZBOGACENIE `compute_soft_signals` O DANE Z FINGERPRINTA

### Plik: `ghost-launcher/src/components/gatekeeper.rs`
### Funkcja: `fn compute_soft_signals`

**UWAGA na zmienne nazwy:**
- Zmienna lokalna to `ss` (nie `soft_signals`)
- `dev_paperhand_latency_ms` to `Option<u64>` (nie f64) — porównanie z `2575_u64`
- Pole SoftSignals to `high_burst_ratio` (nie `high_burst`)
- `assessment.early_fingerprint` to `Option<EarlyFingerprintMetrics>` — wymagany `if let`

### Zmiana chirurgiczna — dodaj NA KOŃCU funkcji, tuż przed `ss`:

```rust
fn compute_soft_signals(&self, assessment: &GatekeeperAssessment) -> SoftSignals {
    let cfg = &self.config;
    let mut ss = SoftSignals::default();

    // ... istniejący kod phase2 velocity, phase3 diversity, dust_filtered ...
    // (nie ruszamy — nadal działa i daje punkty)

    // === WZBOGACENIE O DANE Z EARLY FINGERPRINT ===
    // Dane te są zawsze populowane przez gRPC/Yellowstone pipeline.
    // Jeśli early_fingerprint jest None (brak gRPC), sekcja jest pominięta —
    // istniejące phase2/3 flagi nadal działają jako fallback.
    if let Some(ref fp) = assessment.early_fingerprint {

        // Front-load sniper detection — blok 0 przechwycony przez snajperów
        // Conf z raportu: 0.228. Łączymy OR z istniejącym cabal_suspicion (hhi).
        // Fail-safe: BRAK danych (None) = pass (nie karzemy za brak RPC).
        // Pokrycie danych: 100% w audycie 4488 rekordów.
        if let Some(block0) = fp.block0_sniped_supply_pct {
            if block0 >= 0.117009 {
                ss.cabal_suspicion = true;
            }
        }

        // Early slot volume dominance — boty skupujące early slots
        // Conf z raportu: 0.219. Łączymy OR z istniejącym bundle_suspicion.
        // Pokrycie danych: 100%.
        if let Some(early_dom) = fp.early_slot_volume_dominance_buy {
            if early_dom >= 0.2743 {
                ss.bundle_suspicion = true;
            }
        }

        // Dev paperhand — dev dumps w <2575ms od launchu
        // Conf z raportu: 0.152. Mapujemy na high_burst_ratio (grupa Timing, w=15).
        // FAIL-SAFE CELOWO ŁAGODNY: None = 70.5% rekordów (dev nieznany lub brak danych).
        // Nie karzemy za brak identyfikacji deva — zbyt duże ryzyko false positive.
        // Dev kontrolowany przez istniejący HF-1 (reject_on_dev_sell) gdy włączony.
        if let Some(latency_ms) = fp.dev_paperhand_latency_ms {
            if latency_ms < 2575_u64 {
                ss.high_burst_ratio = true;
            }
        }
    }

    ss
}
```

### Dlaczego OR a nie nadpisanie:
Istniejące flagi (`cabal_suspicion` z HHI, `bundle_suspicion` z same_ms_tx_ratio) już działają po odmrożeniu configu w Kroku 1. Fingerprint-based flagi **rozszerzają** wykrywanie o przypadki których phase3 nie łapie (snajperzy z własnymi walletami, wczesna dominacja slots przed statystycznym oknem GK).

---

## 6. KOLEJNOŚĆ EGZEKUCJI I WARUNKI BEZPIECZEŃSTWA

```
Krok 1: Config (TERAZ)
  └─ Commit: ghost_brain_config.toml
  └─ Czas: 10 minut
  └─ Ryzyko: ZERO — jedyne ryzyko to że za dużo dobrego volume wyleci
  └─ Rollback: zmień max_soft_points = 255

Krok 2: Walidacja shadow (PO 2h RUNU)
  └─ Sprawdź REJECT_SOFT_EXCESS rate
  └─ Sprawdź rozkład soft_points wśród BUY
  └─ Jeśli >40% BUY-ów leci REJECT → podnieś max_soft_points do 80-90

Krok 3: Kod (PO WALIDACJI KROKU 2)
  └─ Plik: ghost-launcher/src/components/gatekeeper.rs
  └─ Funkcja: compute_soft_signals (~linia 2812)
  └─ Cargo build --release
  └─ Shadow run 1h z nowym binarem przed live
```

---

## 7. CZEGO NIE ROBIMY W TYM PLANIE

| Zakaz | Powód |
|---|---|
| Nie wyrzucamy `dev_has_sold` z HF | `dev_paperhand_latency_ms` ma 70.5% null — za mała pokrywa |
| Nie używamy `None => true` dla `dev_paperhand` | 70.5% null → 70.5% puli dostałoby karę za brak danych RPC |
| Nie ruszamy PR3-PR8 refaktor | Niezwiązane, drogie, ryzykowne, skaluje problem a nie go nie naprawia |
| Nie refaktorujemy `SoftSignals` struct | Mamy 11 pól z semantyką wystarczającą do nadpisania |
| Nie zmieniamy `gatekeeper_policy.rs` | `compute_soft_signals` jest w `gatekeeper.rs` — nie mieszamy plików |
| Nie deployujemy Kroku 3 bez shadow walidacji | Krok 1 sam w sobie może być wystarczający |

---

## 8. EXIT CRITERIA

**Krok 1 zaliczony gdy:**
- `logs/decisions.jsonl/gatekeeper_v2_decisions.jsonl` zawiera rekordy z `verdict_type = "REJECT_SOFT_EXCESS"`
- `soft_flags` w logach pokazuje `bundle|cabal|top3` (nie tylko `none`)

**Krok 3 zaliczony gdy:**
- `soft_flags` zawiera wpisy powiązane z fingerprint-derived flags (np. `cabal` aktywowany przez `block0_sniped_supply_pct = 0.31`)
- Null-rate dla `block0` i `early_slot` w nowym runie ≤ 5%
- Shadow P&L nie spada o więcej niż 30% vs baseline z ostatnich 48h

**Rollback trigger:**
- REJECT_SOFT_EXCESS > 70% decyzji → `max_soft_points = 90` (poluzowanie, nie wyłączenie)
- `shadow_run` P&L spada o >50% → `max_soft_points = 255` (pełny rollback, wyjaśnić dlaczego)

---

## 9. DODATKOWE OBSERWACJE Z AUDYTU DANYCH

Dane z `gatekeeper_v2_buys.jsonl` (4488 rekordów) ujawniają skalę problemu:

- **50.9%** obecnych BUY-ów miało `block0_sniped_supply_pct ≥ 0.117009`
- **76.7%** obecnych BUY-ów miało `early_slot_volume_dominance_buy ≥ 0.2743`
- **51.8%** obecnych BUY-ów miało `same_ms_tx_ratio ≥ 0.233`

Te liczby oznaczają, że **skażenie obecnego zbioru BUY jest drastyczne**. Nie jest to wina parsera ani danych — jest to efekt braku aktywacji Layer 3. System przez cały czas zbierał te sygnały i sumiennie logował do JSONL, ale nie działał na ich podstawie.

Po odmrożeniu znaczna część tego 50-77% volume zostanie odrzucona. Jest to **zamierzony efekt** — lepsza qualność wejść kosztem ilości.
