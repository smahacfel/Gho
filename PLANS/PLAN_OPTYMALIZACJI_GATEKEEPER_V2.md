# PLAN OPTYMALIZACJI GATEKEEPER V2 – TUNING PERFORMANCE I SELEKTYWNOŚCI

> **Wersja planu**: 1.0  
> **Data**: 2026-04-29  
> **Kontekst**: Bazowy pipeline Gatekeeper V2 (tryb `long`, konfiguracja `ghost_brain_config.toml` v11) działa zgodnie z audytem `AUDYT_PIPELINE_GATEKEEPER_V2.md`.  
> **Cel**: Przyspieszenie cyklu decyzyjnego, zwiększenie liczby poprawnych werdyktów BUY, ograniczenie false‑negative, przy zachowaniu akceptowalnego poziomu false‑positive.  
> **Uwaga**: Plan nie powiela, nie zastępuje i nie wchodzi w konflikt z programem naprawczym `PLANS/PLAN_WYKONAWCZY.md` – dotyczy wyłącznie strojenia warstwy decyzyjnej po ustabilizowaniu ingestu i runtime.

## 1. Obecny stan – identyfikacja wąskich gardeł

### 1.1. Timery i progi ilościowe

Z pliku `ghost_brain_config.toml` (sekcja `[gatekeeper_v2]`):

| Parametr | Aktualna wartość | Efekt |
|----------|------------------|-------|
| `max_wait_time_ms` | **8001** | Bardzo długie okno – pula czeka >8 s na decyzję; może powodować opóźnienia w zatłoczonych strumieniach. |
| `min_tx_count` | **40** | Wysoki próg – większość pul nie osiąga 40 TX w 8 s → **TIMEOUT_PHASE1**. |
| `min_unique_signers` | **32** | Jeszcze ostrzejszy niż liczba TX – istotnie ogranicza BUY. |
| `min_buy_count` | **32** | Podobnie. |

**Konsekwencje**: Tokeny o dobrym potencjale, ale umiarkowanej aktywności (20‑30 TX, 15‑20 unikalnych portfeli) przechodzą w `TIMEOUT`, mimo że fazy 2‑6 wskazują na organiczny ruch. Fenomen ten widoczny jest w logach decyzji (duży odsetek timeoutów).

### 1.2. Filtr twardy MarketCap

`min_market_cap_sol = 60` – w połączeniu z innymi cięciami twardymi (obecnie aktywny tylko MarketCap i SlowPool) eliminuje pulę, która zgromadziła realny kapitał ~40‑50 SOL. Jednocześnie dev‑unknown zaostrza ten sam próg do 60 SOL (przez `dev_unknown_min_market_cap_sol`).  
**Propozycja**: segmentacja – osobny łagodniejszy próg dla pierwszych 2 s (kapitalizacja wstępna może być niższa).

### 1.3. Soft scoring (❄ FROZEN)

`max_soft_points = 255`, praktycznie wyłączony. System nie odrzuca pul za lekkie anomalie timing‑owe / bot‑like sygnały. W obecnym stanie daje to bezpieczeństwo (brak fałszywych odrzuceń), ale kosztem selektywności – pule z wyraźnymi oznakami manipulacji (np. high burst_ratio) przechodzą dalej, jeżeli progi Core są spełnione.

### 1.4. Alpha Gate i Prosperity – parametry

Alpha Gate (`min_momentum = 0.2, min_demand = 0.2, min_alpha_joint = 0.2`) może odrzucać tokeny o ledwo przekroczonych progach Core. Prosperity Filter z trzema wąskimi gałęziami (Branch1: sniper≥28% + sell/buy≤16%, Branch2: mcap≥55 + early_dom≥90%, Branch3: HHI≤0.0416 + FTDI≥0.0909) jest ultra‑selektywny i w obecnym kształcie prawdopodobnie przepuszcza minimalną liczbę pul.

## 2. Cele optymalizacji

1. **Zwiększenie liczby BUY dla organicznych, wcześnie‑rosnących tokenów**, szczególnie tych z 20‑35 TX w oknie.
2. **Skrócenie czasu decyzji** – zmniejszenie `max_wait_time_ms` do 5000 ms, by szybciej zwalniać zasoby i szybciej reagować na zmiany rynku.
3. **Ograniczenie false‑positive** – nie dopuścić do przejścia ewidentnych bot‑farm / single‑whale pools.
4. **Zachowanie możliwości ręcznego cofnięcia** – wszystkie zmiany realizowane wyłącznie przez konfigurację, bez zmian w kodzie Rust; rollback przez przywrócenie poprzedniego TOML.

## 3. Proponowane zmiany w `ghost_brain_config.toml`

### 3.1. Skrócenie okna i obniżenie progów ilościowych

```toml
[gatekeeper_v2]
max_wait_time_ms = 5000      # z 8001
min_tx_count     = 30        # z 40
min_unique_signers = 24      # z 32
min_buy_count    = 24        # z 32
```

**Uzasadnienie**: Statystyki z logów (TIME‑OUT) pokazują medianę TX dla pul, które ostatecznie osiągają BUY (w późniejszym horyzoncie) na poziomie 28‑35. 30 TX w 5 s jest realistyczne dla dobrej trakcji.

### 3.2. Obniżenie floor market‑cap dla wczesnej fazy

Wprowadzenie mechanizmu **dwustopniowego market cap** (przez zmianę `dev_unknown_min_market_cap_sol` i dodanie nowego parametru? Obecnie architektura może wymagać modyfikacji kodu, więc ograniczamy się do bezpiecznych zmian w istniejących polach:

```toml
min_market_cap_sol = 35      # z 60 – ogólny próg dla pul z potwierdzoną krzywą
dev_unknown_min_market_cap_sol = 35  # z 60 – wyrównanie dla dev‑unknown
```

**Uwaga**: aby to zadziałało należy potwierdzić, że niższa kapitalizacja nie jest odrzucana przez Prosperity Filter (obecnie wymaga ≥45 SOL). Jeśli tak, dostosujemy Prosperity.

### 3.3. Rozluźnienie Prosperity Filter

Aby nie być wąskim gardłem po obniżeniu progów market cap, zmieniamy progi Prosperity:

```toml
enable_prosperity_filter = true
prosperity_min_market_cap_sol = 25        # z 45
prosperity_branch2_min_market_cap_sol = 30 # z 55
prosperity_branch2_min_early_slot_volume_dominance_buy = 0.70  # z 0.90
prosperity_branch1_min_block0_sniped_supply_pct = 0.20       # z 0.28
prosperity_branch3_max_hhi = 0.15          # z 0.0416 (zezwala na nieco większą koncentrację)
```

### 3.4. Aktywacja soft scoring (odmrożenie)

Włączenie warstwy soft signal jako dodatkowego filtru przy zmniejszonych wymaganiach Core:

```toml
max_soft_points = 8             # z 255 → realny limit
soft_weight_timing       = 1
soft_weight_manipulation = 2
soft_weight_diversity    = 1
soft_weight_ecosystem    = 1
```

Dodatkowo sybil soft points:

```toml
max_sybil_soft_points = 4        # z 6
```

### 3.5. Obniżenie alpha gate (pozytywny selektor)

Aby nie odcinać tokenów o słabym momentum przy krótkim oknie:

```toml
min_momentum = 0.15      # z 0.2
min_demand   = 0.15
min_alpha_joint = 0.15
```

### 3.6. Reaktywacja IWIM Veto Gate (jako bezpiecznik)

Gdy poluzujemy progi Core, warto dorzucić dodatkową warstwę bezpieczeństwa opartą o historię dewelopera:

```toml
[iwim_veto_gate]
enabled = true
mode = "grpc"
max_wait_ms = 800
min_confidence = 0.55
rug_threat_threshold = 0.75
sybil_threshold = 0.70
organic_floor = 0.10
```

**Konieczność**: funkcja IWIM Veto jest zaimplementowana, ale wyłączona – jej aktywacja nie wymaga zmian kodu, jedynie konfiguracji.

## 4. Oczekiwany wpływ na parametry operacyjne

| Metryka | Przed | Po (szacunek) |
|---------|-------|---------------|
| Średni czas do decyzji | ~8.2 s | <5.5 s |
| Odsetek BUY wśród ocenionych pul | ~4% | 12–18% |
| Odsetek false‑positive (boty/single‑whale) | <1% | wciąż <2% dzięki IWIM+soft scoring |
| Zużycie CPU (liczba buforowanych pul) | bez zmian | potencjalnie niższe, bo krótsze życie puli |

## 5. Plan testów

### 5.1. Warstwa testów jednostkowych

- `gatekeeper.rs` – istniejące testy pokrywają logikę faz; należy uruchomić cały zestaw z nowym configiem, aby upewnić się, że zmiana progów nie powoduje paniki (np. dzielenie przez zero w wyliczeniach faz).
- Dodatkowy test: akceptacja puli przy 30 TX, 24 unikalnych signerach – symulować scenariusz i potwierdzić, że decyzja to BUY (a nie TIMEOUT).

### 5.2. Testy integracyjne (shadow‑burnin)

Uruchomić system w trybie shadow‑burnin (zapisuje decyzje, ale nie wykonuje transakcji) na historycznych danych z 1 godziny (ok. 300 pul). Porównać:

- Liczbę wygenerowanych BUY,
- Przyczyny odrzuceń,
- Czas do decyzji.

### 5.3. Analiza logów

Przeanalizować wyprodukowane pliki `gatekeeper_v2_decisions.jsonl` i `gatekeeper_v2_buys.jsonl` w poszukiwaniu anomalii: wzrost fałszywych BUY na znanych scam tokenach, nadmierne odrzucenia przez nowy soft scoring, itp.

## 6. Procedura wdrożenia

1. **Backup bieżącego `ghost_brain_config.toml`**.
2. **Wprowadzenie zmian** opisanych w sekcji 3 (edycja jednego pliku).
3. **Uruchomienie testów jednostkowych**:
   ```bash
   cargo test -p ghost-launcher --lib -- gatekeeper
   cargo test -p ghost-brain --lib -- config
   ```
4. **Uruchomienie shadow‑burnin** na danych historycznych – 1 h.
5. **Weryfikacja logów** – akceptacja BUY dla pul z wcześniejszymi TIME‑OUT, brak BUY dla oczywistych bot‑farm.
6. **Decyzja** – jeśli wyniki satysfakcjonujące, wdrożyć na live (przełączenie configu w `config.toml`).

## 7. Odpowiedzialność i cofanie

- Każda zmiana jest odwracalna przez przywrócenie starego `ghost_brain_config.toml`.
- Testy przedwdrożeniowe minimalizują ryzyko regresji.
- Plan nie ingeruje w architekturę ani w kod – jest czysto konfiguracyjny.

---

**Koniec planu optymalizacji Gatekeeper V2.**
