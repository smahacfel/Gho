# ADR-0119: Odblokowanie Gatekeeper Phase 4/5/6 — poluzowanie fingerprint thresholds + diagnostyka config loadingu

**Data**: 2026-04-29  
**Status**: Zaakceptowane  
**Autor**: Ghost Father (diagnostyka i naprawa)

---

## Decyzja

1. **Poluzowano fingerprint thresholds** w `ghost_brain_config.toml` — były ukrytym gate'm blokującym Phase 4 nawet przy minimalnych progach głównych
2. **Wyłączono Alpha Gate, Prosperity Filter, Sybil Interference Layer** do czasu rekalibracji na nowych progach
3. **Phase 4/5/6 główne progi** ustawione na absolutne minimum (0.0)
4. **Dodano diagnostykę config loadingu** — ścieżka configa w CONFIG fingerprint + eprintln przed init_logging

## Kontekst

Użytkownik zgłosił trzy problemy:

### Problem 1: Bot nie startuje — grpc_endpoint = localhost:10000

Log pokazuje, że `config.toml` nie był wczytywany — wszystkie wartości były domyślne (`execution_mode=Live`, `entry_mode=live`, `grpc_endpoint=http://localhost:10000`). Walidacja `validate_grpc_config()` poprawnie odrzucała placeholder.

**Root cause**: `resolve_config_path` nie znajdował `config.toml` w katalogu roboczym. Użytkownik prawdopodobnie uruchamiał bota z innego katalogu bez flagi `--config`.

### Problem 2: Pule nie przechodzą przez fazy 4,5,6 mimo minimalnych progów

Analiza logów z działającego bota (11:18-11:35, 2026-04-29) wykazała:

1. **HARD_FAIL: market_cap < 60.0** — aktywny próg `min_market_cap_sol = 60.0` z poprzedniej konfiguracji. Bot używał starego configu wczytanego przy starcie — zmiany na dysku nie były aktywne bez restartu.

2. **Fingerprint thresholds** (`phase4_fingerprint_thresholds_pass` w `gatekeeper_policy.rs:1594-1622`) były dodatkowym, ukrytym gate'm na Phase 4:
   - `max_sell_buy_ratio = 0.4` — blokował pule z normalną sprzedażą
   - `min_compute_unit_cluster_dominance = 0.40` — wymuszał dominację klastra CU
   - `max_fixed_size_buy_ratio = 0.089` — blokował boty z fixed-size buys
   - `max_early_top3_buy_volume_pct_3s = 0.71` — blokował skoncentrowane wczesne zakupy

   Te progi były w sekcji "Hybrid gRPC / Yellowstone fingerprint thresholds", osobnej od głównych progów Phase 4. Użytkownik nie wiedział o ich istnieniu.

3. **Phase 4/5/6 pokazywane jako ❌ w logach** — wynikało to z HARD_FAIL (market cap) odpalającego się przed sprawdzeniem faz (kolejność: hard fails → core1 → core2 → core3).

### Problem 3: `gatekeeper_v2_buys.jsonl` nie powstaje

Plik `buys.jsonl` jest tworzony tylko gdy `decision_verdict_buy == Some(true)` (`decision_logger.rs:1764`). Ponieważ żadna pula nie przechodziła do werdyktu BUY (wszystkie HARD_FAIL na market cap), plik nie mógł powstać.

## Zmiany

### 1. `ghost-brain/ghost_brain_config.toml`

#### Fingerprint thresholds (sekcja "Hybrid gRPC / Yellowstone"):
| Klucz | Stara wartość | Nowa wartość |
|-------|--------------|-------------|
| `max_sell_buy_ratio` | 0.4 | 0.99 |
| `min_compute_unit_cluster_dominance` | 0.40 | 0.0 |
| `min_avg_inner_ix_count_50tx` | 0.01 | 0.0 |
| `min_fixed_size_buy_ratio` | 0.0001 | 0.0 |
| `max_fixed_size_buy_ratio` | 0.089 | 0.99 |
| `max_early_top3_buy_volume_pct_3s` | 0.71 | 0.99 |

#### Phase 4 — Volume Sanity:
| Klucz | Stara wartość | Nowa wartość |
|-------|--------------|-------------|
| `min_buy_ratio` | 0.80 | 0.0 |
| `max_sol_buy_ratio` | 0.96 | 1.0 |
| `min_avg_tx_sol` | 0.01 | 0.0 |
| `min_volume_cv` | 0.01 | 0.0 |
| `min_total_volume_sol` | 1.0 | 0.0 |
| `min_consecutive_buys` | 1 | 0 |

#### Phase 6 — Bonding Curve:
| Klucz | Stara wartość | Nowa wartość |
|-------|--------------|-------------|
| `min_price_change_ratio` | 0.01 | 0.0 |
| `max_bonding_progress_pct` | 99.0 | 100.0 |
| `min_bonding_progress_pct` | 4.0 | 0.0 |
| `min_market_cap_sol` | 4.0 | 0.0 |

#### Wyłączone selektory (do rekalibracji):
| Klucz | Stara wartość | Nowa wartość |
|-------|--------------|-------------|
| `enable_alpha_gate` | true | false |
| `enable_prosperity_filter` | true | false |
| `enable_sybil_interference_layer` | true | false |
| `mode` | "long" | "standard" |
| `dev_unknown_min_market_cap_sol` | 1.0 | 0.0 |
| `max_sybil_soft_points` | 6 | 255 |

### 2. `ghost-launcher/src/main.rs`

Dodano `eprintln!` przed `init_logging()` aby zawsze pokazywać ścieżkę configu na stderr:
```rust
eprintln!("[ghost] Loading configuration from: {}", config_path.display());
```

### 3. `ghost-launcher/src/config.rs`

Dodano `brain_cfg={}` do CONFIG fingerprint logu dla łatwiejszej diagnostyki.

## Konsekwencje

### Pozytywne
- Pule powinny przechodzić przez fazy 4,5,6 i docierać do werdyktu BUY
- `gatekeeper_v2_buys.jsonl` powinien się tworzyć przy pierwszych decyzjach BUY
- Diagnostyka config loadingu pozwoli szybciej wykryć problemy ze ścieżkami

### Negatywne
- **Bardzo wysoki false-positive rate** — bot będzie akceptował praktycznie każdą pulę
- Alpha Gate, Prosperity Filter, Sybil wyłączone — brak dodatkowej filtracji
- Konieczna rekalibracja po zebraniu danych shadow-burnin

### Plan przywracania
1. Uruchom shadow-burnin z poluzowanymi progami → zbierz dane
2. Przeanalizuj `gatekeeper_v2_decisions.jsonl` pod kątem dystrybucji metryk
3. Skalibruj progi na podstawie danych (osobno fingerprint, Phase 4, Phase 6)
4. Włącz Alpha Gate → test
5. Włącz Prosperity Filter → test
6. Włącz Sybil → test

## Uruchomienie

Aby bot działał poprawnie, uruchom go z katalogu `/root/Gho`:
```bash
cd /root/Gho
cargo run --release -- --config configs/rollout/shadow-burnin.toml
```

Alternatywnie, użyj głównego `config.toml`:
```bash
cd /root/Gho
cargo run --release
```

Ścieżka do configu będzie teraz widoczna na stderr przed inicjalizacją logowania.
