Niniejszy dokument zawiera zestaw 6 nowych metryk o dużej wartości, o które powinien zostać wzbogacony tx intelligence, po ustabilizowaniu runtime Ghosta. 

## Spis metryk

| # | Nazwa | Skrót | Wektor detekcji | Wymiar |
|---|---|---|---|---|
| 1 | Fee Topology Diversity Index | FTDI | infrastruktura kupujących | toolchain |
| 2 | Dev-Buyer Infrastructure Affinity | DBIA | powinowactwo dev↔buyer | toolchain |
| 3 | Spend Fraction Divergence | SFD | wzorzec alokacji kapitału | ekonomia |
| 4 | Funding Source Concentration | FSC | łańcuch finansowania | funding |
| 5 | Signer Cross-Pool Velocity | CPV | behawior cross-pool | behawior |
| 6 | Demand Elasticity Score | DES | reakcja rynku na cenę | dynamika |

---

## Kluczowe ustalenia architektoniczne z sesji

Przed opisem metryk — fundamentalne fakty które wielokrotnie eliminowały błędne propozycje:

### 1. Path Independence na CPMM (Constant Product Market Maker)
Na bonding curve pump.fun (x·y=k) podział transakcji na N kawałków w tym samym kierunku, bez interweniujących transakcji od innych uczestników, daje **dokładnie tyle samo tokenów** co jedna duża transakcja. Dowód algebraiczny: środkowe wyrazy rezerw skracają się teleskopowo. Konsekwencja: metryki oparte na "koszcie fragmentacji na curve" (np. BCE) są martwe — zawsze dają wynik 1.0.

### 2. Geyser emituje post-execution, nie post-submission
Yellowstone Geyser jest wtyczką wewnątrz rdzenia walidatora. `SubscribeUpdateTransaction` emituje zdarzenia **w momencie przetwarzania bloku**, nie w momencie dotarcia transakcji do mempoola/TPU. Konsekwencja: arrival timestamps z gRPC receivera nie niosą informacji o origin latency kupujących (Tokyo vs Frankfurt). Zarówno bundle Jito jak i organiczni snajperzy w tym samym slocie dają jitter <0.5ms na receiverze.

### 3. Każdy buyer tworzy ATA w pierwszych 8s
Nowy mint powstaje w momencie CreatePool. Nikt nie posiada ATA dla tego tokena przed pool creation. Każda transakcja buy (organic i cabal) zawiera `CreateAssociatedTokenAccount`. Metryki oparte na "fresh wallet = tworzy ATA" są bezużyteczne w oknie t0.

### 4. Sample size N=5 jako twardy constraint
Przy 5 transakcjach i 5 signerach: entropia Shannona max ≈ 2.32, PCA ma max 4 niezerowe eigenvalues, regresja z >3 parametrami jest underdetermined, finite differences Δy/Δx eksplodują przy małych Δx. Każda metryka musi być walidowana mentalnie na N=5 zanim wejdzie do specyfikacji.

---

## M1. Fee Topology Diversity Index (FTDI)

### Co mierzy
Różnorodność infrastruktury (toolchain) używanej przez kupujących, na podstawie strukturalnych cech transferów SOL wewnątrz transakcji — bez znajomości konkretnych adresów botów.

### Dlaczego to jest potrzebne
Wash-trading z jednego skryptu generuje transakcje o identycznej topologii fee. Organic retail używa różnych botów (Trojan, Photon, BullX, pump.fun UI), z których każdy ma inną strukturę opłat. FTDI łapie tę różnicę bez hardcoded whitelisty adresów.

### Źródło danych (gRPC)
- `SubscribeUpdateTransaction` → `TransactionStatusMeta.inner_instructions`
- Wyciągnięcie wszystkich instrukcji `SystemProgram::Transfer` z inner instructions
- `account_keys` transakcji do klasyfikacji destination accounts

### Algorytm

**Krok 1: Klasyfikacja transferów per transakcja**

Dla każdego buy_tx w oknie 8s iterujesz po `inner_instructions` i wyciągasz wszystkie `SystemProgram::Transfer`. Dla każdego transferu klasyfikujesz destination:

- `destination ∈ pool_accounts` (pool state, bonding curve vault, fee account pump.fun) → **internal transfer**
- `destination ∉ pool_accounts` i `destination ≠ signer` i `destination ≠ signer_wsol_ata` → **external fee transfer**

**⚠️ KRYTYCZNE OSTRZEŻENIE IMPLEMENTACYJNE:**  
Podczas swapów na pump.fun występuje pakowanie/rozpakowywanie natywnego SOL do/z WSOL (Wrapped SOL). Token Program wykonuje transfery między kontem Signera a jego własnym WSOL ATA. Algorytm **MUSI** filtrować te transfery transformacyjne przed budową sygnatury. Transfer z signera na jego własne WSOL ATA (lub odwrotnie) **NIE JEST** external fee.

**Krok 2: Budowa sygnatury topologicznej**

```
topology(tx) = (N_external_fees, N_internal_fees)
```

Przykłady sygnatur dla znanych narzędzi (referencyjne, NIE hardcoded):
- pump.fun web UI: `(0, 0)` — zero dodatkowych transferów
- Trojan Bot: `(1, 0)` — jeden external fee (prowizja platformy)
- Photon: `(1, 0)` — jeden external fee
- BullX: `(2, 0)` — dwa external fees (referral + platform)
- Custom script deva: `(0, 0)` lub `(0, 1)` — brak prowizji lub self-transfer

**Krok 3: Obliczenie FTDI**

```
unique_topologies = count_distinct(topology[tx] for tx in buy_txs)
ftdi = unique_topologies / unique_signers_evaluated
```

### Interpretacja

| Wartość | Znaczenie |
|---|---|
| FTDI ≈ 0.2 | Niska dywersyfikacja — wszyscy kupujący używają tego samego toolchainu. Potencjalny wash-trade. |
| FTDI ≈ 0.4–0.6 | Umiarkowana dywersyfikacja — mix narzędzi. Typowe dla organic. |
| FTDI ≈ 0.8–1.0 | Wysoka dywersyfikacja — każdy buyer z innym narzędziem. Silny sygnał organic. |

### Znane ograniczenia
- Jeśli 5 organicznych retailowców wszystkich używa Trojana → FTDI = 0.2 (false positive). Dlatego soft signal, nie hard gate.
- Synergia z DBIA: jeśli FTDI niskie ale DBIA też niskie → wspólny retail bot, nie cabal. Cross-korelacja DBIA × FTDI rozwiązuje ten case.

### Konfiguracja

```toml
min_fee_topology_diversity_index = 0.25
soft_penalty_low_ftdi = 4
```

### Struct

```rust
pub fee_topology_diversity_index: Option<f64>,
```

### Warunek degradacji
- Jeśli `buy_count < 3` → `None`, degraded_reason += `"FTDI_INSUFFICIENT_BUYS"`

---

## M2. Dev-Buyer Infrastructure Affinity (DBIA)

### Co mierzy
Stopień podobieństwa infrastrukturalnego (toolchain/software) między transakcją kupna dewelopera a transakcjami kupna pozostałych buyerów. Wysoka afinitas = podejrzenie że dev i buyerzy używają tego samego codebase.

### Dlaczego to jest potrzebne
Istniejące metryki porównują buyerów **do siebie nawzajem** (FTDI, HHI, Gini). Żadna nie porównuje buyerów **do deva**. Cabal operator tworzy pool z custom Rust scriptu i obsługuje cabal wallety tym samym codebase — generując identyczną strukturę transakcji. DBIA wykrywa tę asymetryczną relację.

### Źródło danych (gRPC)
- `SubscribeUpdateTransaction` → `transaction.message.instructions` (outer instructions)
- `TransactionStatusMeta.inner_instructions`
- `transaction.message.account_keys` z flagami `is_signer`, `is_writable`
- Identyfikacja dev_pubkey z CreatePool/Initialize event (już istnieje w pipeline)

### Algorytm

**Krok 1: Budowa fingerprintu infrastrukturalnego per transakcja**

```
fingerprint(tx) = (
    len(account_keys),                      // u8 — ilość kont w tx
    len(instructions),                      // u8 — ilość outer instructions
    has_set_compute_unit_limit,             // bool
    has_set_compute_unit_price,             // bool
    count(inner_instructions_groups),       // u8 — ilość grup inner ix
    fee_topology                            // (u8, u8) — z FTDI
)
```

Dane pobierane z: outer `instructions` (count + ComputeBudget detection), `inner_instructions` (count), `account_keys` (length). Wszystko dostępne w `SubscribeUpdateTransaction`.

**Krok 2: Fingerprint dev buy transaction**

Identyfikuj transakcję kupna deva (signer == dev_pubkey, direction == BUY). Oblicz `dev_fp = fingerprint(dev_buy_tx)`.

Jeśli dev nie kupił w oknie → DBIA = None (nie można obliczyć).

**Krok 3: Weighted distance zamiast binary match**

❌ **NIE**: `count(fp == dev_fp) / N` — zbyt kruche, drobne różnice (wersja bota, CU price) rozbijają matching.

✅ **TAK**: Weighted Hamming distance z feature importances:

```
weights = {
    account_keys_len:           0.20,
    instructions_len:           0.25,
    has_set_cu_limit:           0.05,
    has_set_cu_price:           0.05,
    inner_ix_group_count:       0.25,
    fee_topology:               0.20,
}

distance(fp_a, fp_b) = sum(
    weight[f] * (1 if fp_a[f] != fp_b[f] else 0)
    for f in features
)

similarity(fp_a, fp_b) = 1.0 - distance(fp_a, fp_b)
```

Uzasadnienie wag:
- `has_set_cu_limit` i `has_set_cu_price` mają niską wagę — w pierwszych 8s na pump.fun prawie wszyscy snajperzy ustawiają maxed out priority fees (`cu_price_p90_1s = 1_000_000`), więc te cechy mają niską dyskryminacyjność.
- `instructions_len`, `inner_ix_group_count`, `fee_topology` mają wysoką wagę — silnie zależą od toolchainu, trudne do randomizacji bez zmiany softu.

**Krok 4: Obliczenie DBIA**

```
buyer_fps = [fingerprint(tx) for tx in non_dev_buy_txs]
dbia = mean(similarity(dev_fp, fp) for fp in buyer_fps)
```

### Interpretacja

| Wartość | Znaczenie |
|---|---|
| DBIA ≈ 0.0–0.3 | Buyerzy używają innego toolchainu niż dev. Organic. |
| DBIA ≈ 0.4–0.6 | Częściowe pokrycie. Wymaga analizy w kontekście FTDI. |
| DBIA ≈ 0.7–1.0 | Buyerzy i dev na tym samym lub bardzo podobnym software. Silny sygnał cabal. |

### Cross-korelacja z FTDI (kluczowa)
| DBIA | FTDI | Interpretacja |
|---|---|---|
| Wysokie | Niskie | **Cabal pewniak** — wszyscy (łącznie z devem) na tym samym skrypcie |
| Wysokie | Wysokie | **Shared retail bot** — dev i buyers obaj na Trojanie, nie cabal |
| Niskie | Niskie | **Homogeniczny retail** — jeden popularny bot, dev na czymś innym |
| Niskie | Wysokie | **Zdrowy organic** — różne narzędzia, dev osobno |

### Konfiguracja

```toml
max_dev_buyer_infrastructure_affinity = 0.60
soft_penalty_high_dbia = 7
```

### Struct

```rust
pub dev_buyer_infrastructure_affinity: Option<f64>,
```

### Warunki degradacji
- Dev nie kupił w oknie → `None`, degraded_reason += `"DBIA_NO_DEV_BUY"`
- `buy_count < 2` (poza devem) → `None`, degraded_reason += `"DBIA_INSUFFICIENT_BUYERS"`

---

## M3. Spend Fraction Divergence (SFD)

### Co mierzy
Rozrzut frakcji portfela wydanej przez każdego kupującego. Cabal wallety fundowane "pod korek" wydają ~90% salda. Organic buyerzy wydają losowy procent swoich środków.

### Dlaczego to jest potrzebne
Istniejące metryki mierzą **ile** kto wydał (`volume_cv`) lub **ile** kto ma (`pre_balance`). SFD mierzy **jaką frakcję** posiadanych środków kto zainwestował — to jest capital deployment pattern, nie kwota ani saldo.

Kluczowa przewaga nad pre_balance CV: dodanie dustu do portfela (koszt: ≈0) podnosi pre_balance CV. Ale żeby obniżyć spend fraction, cabal musi **zamrozić realny kapitał** w portfelu — to kosztuje.

### Źródło danych (gRPC)
- `TransactionStatusMeta.pre_balances[]` — saldo signera przed transakcją (w lamportach)
- `TransactionStatusMeta.post_balances[]` — saldo signera po transakcji (w lamportach)
- Indeksowane po pozycji w `account_keys`, signer ma flagę `is_signer = true`

### Algorytm

**Krok 1: Obliczenie spend fraction per buyer**

```
spend_fraction[i] = (pre_balances[signer_idx] - post_balances[signer_idx]) / pre_balances[signer_idx]
```

Ignoruj transakcje gdzie `pre_balances[signer_idx] == 0` (nie powinno się zdarzyć, ale edge case).

**Krok 2: Obliczenie SFD jako MAD (Median Absolute Deviation)**

❌ **NIE CV** — wrażliwe na single outlier (jeden whale z fraction 0.01 wśród degenów z 0.8 wysadza CV).

✅ **MAD** — robustna na outliers:

```
median_frac = median(spend_fractions[])
mad = median(|spend_fractions[i] - median_frac| for all i)
sfd = mad
```

**Krok 3: Ważenie po sqrt(buy_amount)**

❌ **NIE `weight = buy_amount / total_volume`** — whale z 3 SOL przy total 5.5 SOL zdominuje metrykę.

✅ **`weight = sqrt(buy_amount)`** — kompromis między equal weighting a volume weighting:

```
weights[i] = sqrt(buy_amount_sol[i])
weighted_median_frac = weighted_median(spend_fractions[], weights[])
weighted_mad = weighted_median(|spend_fractions[i] - weighted_median_frac|, weights[])
sfd = weighted_mad
```

Jeśli weighted median jest zbyt złożony implementacyjnie na hot path, standardowy MAD bez ważenia jest akceptowalny jako v1.

### Interpretacja

| Wartość | Znaczenie |
|---|---|
| SFD ≈ 0.0–0.05 | Wszyscy kupujący wydają zbliżoną frakcję portfela. Wallets fundowane "pod korek". Cabal. |
| SFD ≈ 0.1–0.3 | Umiarkowany rozrzut. Szara strefa — potrzebne inne sygnały. |
| SFD ≈ 0.3+ | Wysoki rozrzut frakcji. Organic — mix whales, degenów i drobnych. |

### Przykład liczbowy

**Cabal (5 wallets fundowanych po ~1.2 SOL, każdy kupuje za ~1.0 SOL):**
```
spend_fractions = [0.88, 0.91, 0.86, 0.93, 0.89]
median = 0.89
MAD = median([0.01, 0.02, 0.03, 0.04, 0.00]) = 0.02
SFD = 0.02  → NISKI → penalty
```

**Organic (5 różnych traderów):**
```
spend_fractions = [0.01, 0.83, 0.20, 0.45, 0.62]
median = 0.45
MAD = median([0.44, 0.38, 0.25, 0.00, 0.17]) = 0.25
SFD = 0.25  → WYSOKI → OK
```

### Konfiguracja

```toml
min_spend_fraction_divergence = 0.08
soft_penalty_low_sfd = 6
```

### Struct

```rust
pub spend_fraction_divergence: Option<f64>,
```

### Warunki degradacji
- `buy_count < 3` → `None`, degraded_reason += `"SFD_INSUFFICIENT_BUYS"`
- Którykolwiek `pre_balance == 0` → pomiń tego signera, dodaj `"SFD_ZERO_PREBALANCE_SKIPPED"`

---

## M4. Funding Source Concentration (FSC)

### Co mierzy
Czy kupujący w oknie obserwacyjnym zostali sfinansowani przez to samo źródło (ten sam wallet nadrzędny). Cabal **musi** fundować wallety — to operacyjna konieczność sybil attacku.

### Dlaczego to jest potrzebne
Żadna istniejąca metryka nie patrzy **skąd** kupujący mają SOL. FTDI i DBIA patrzą na toolchain, SFD na deployment pattern. FSC zamyka łańcuch: jeśli 4 z 5 buyerów dostało SOL z tego samego portfela 3 minuty temu, to cabal.

### Źródło danych (gRPC)
- Rolling state z `SubscribeUpdateTransaction`: wszystkie `SystemProgram::Transfer` observowane w gRPC streamie w ostatnich N minut
- Struktura w pamięci: `HashMap<Pubkey, Vec<(from: Pubkey, amount: u64, ts: u64)>>` z TTL

### Algorytm

**Krok 1: Utrzymywanie rolling transfer log**

Dla każdej transakcji w gRPC streamie (nie tylko na analizowanym poolu):
- Wyciągnij `SystemProgram::Transfer` instrukcje
- Zapisz `(from, to, amount, timestamp)` w rolling state
- TTL: 300s (5 minut) — starcza do pokrycia pre-launch funding

Koszt pamięci: `(Pubkey + Pubkey + u64 + u64) × estimated_transfers_per_5min`. Przy bounded map z eviction — kontrolowalny.

**Krok 2: Lookup funding sources per buyer**

Kiedy Gatekeeper analizuje pool, dla każdego buyer_pubkey w oknie 8s:
```
funding_source[buyer] = lookup(rolling_transfer_log, buyer, window=300s)
    → from_pubkey który wysłał SOL do tego buyera (najpóźniejszy transfer > dust threshold)
```

Jeśli buyer nie ma wpisu w rolling log → `funding_source = UNKNOWN`.

**Krok 3: Obliczenie FSC**

```
known_sources = [funding_source[b] for b in buyers if funding_source[b] != UNKNOWN]

if len(known_sources) < 2:
    return None  // za mało danych

unique_funders = count_distinct(known_sources)
fsc = 1.0 - (unique_funders / len(known_sources))
```

**Krok 4: CEX hot wallet whitelist**

Znane CEX hot wallets (Binance, Coinbase, Kraken — 5-10 adresów, stabilna lista) → klasyfikuj jako `NEUTRAL_FUNDER`. Wielu organicznych traderów jest fundowanych z tego samego Binance hot wallet — to nie jest sygnał cabal.

```
if funding_source[buyer] ∈ CEX_HOT_WALLETS:
    funding_source[buyer] = NEUTRAL_FUNDER

// Przy obliczaniu FSC, NEUTRAL_FUNDER liczy się jako unikalne źródło per buyer
// (nie łączymy dwóch buyerów fundowanych przez Binance w jedną grupę)
```

### Interpretacja

| Wartość | Znaczenie |
|---|---|
| FSC ≈ 0.0 | Każdy buyer z innego źródła (lub unknown). Organic. |
| FSC ≈ 0.3–0.5 | Częściowa koncentracja. Sprawdź inne sygnały. |
| FSC ≈ 0.6–1.0 | Większość buyerów z jednego źródła. Silny sygnał cabal. |

### Znane ograniczenia
- Multi-hop funding (funder → intermediary → buyer) omija single-hop lookup. Zwiększenie głębokości do 2 hops jest możliwe ale kosztowne pamięciowo.
- Jeśli cabal funduje wallety z CEX withdrawal → NEUTRAL_FUNDER → FSC = 0.0 (false negative). Dlatego FSC to soft signal, nie jedyna linia obrony.
- Rolling state wymaga pamięci proporcjonalnej do throughputu sieci. Konieczny cap na rozmiar mapy + eviction policy.

### Konfiguracja

```toml
max_funding_source_concentration = 0.60
soft_penalty_high_fsc = 8
funding_lookback_window_s = 300
funding_dust_threshold_lamports = 10_000_000  # 0.01 SOL
```

### Struct

```rust
pub funding_source_concentration: Option<f64>,
```

### Warunki degradacji
- Mniej niż 2 buyerów z known funding source → `None`, degraded_reason += `"FSC_INSUFFICIENT_KNOWN_SOURCES"`

---

## M5. Signer Cross-Pool Velocity (CPV)

### Co mierzy
Jaki odsetek kupujących na analizowanym poolu pojawił się jako buyer na **innych** poolach w krótkim oknie czasowym. Sybil networks rotują po kolejnych launchach.

### Dlaczego to jest potrzebne
Istniejące metryki operują w ramach jednego poola. CPV łapie korelację **między** poolami — jeśli ten sam wallet kupuje na 4 różnych poolach w ciągu 3 minut, to nie jest organic discovery. To koordynacja.

### Źródło danych (gRPC)
- Rolling state z `SubscribeUpdateTransaction`: `HashMap<Pubkey, Vec<(pool_id: Pubkey, ts: u64)>>` z TTL
- Dane zbierane z transakcji na **wszystkich** monitorowanych poolach pump.fun, nie tylko z analizowanego

### Algorytm

**Krok 1: Utrzymywanie rolling signer activity log**

Dla każdej transakcji buy na jakimkolwiek pool pump.fun:
```
signer_activity_log[signer_pubkey].push((pool_id, timestamp))
```
TTL: 300s. Bounded map z eviction.

**Krok 2: Lookup per buyer na analizowanym poolu**

```
for buyer in current_pool_buyers:
    other_pools = count_distinct(
        pool_id for (pool_id, ts) in signer_activity_log[buyer]
        where pool_id != current_pool_id
        and ts > now - window_s
    )
    is_cross_pool = other_pools > 0
```

**Krok 3: Obliczenie CPV**

```
cpv = count(buyers where is_cross_pool) / unique_signers_evaluated
```

### Interpretacja

| Wartość | Znaczenie |
|---|---|
| CPV ≈ 0.0 | Żaden buyer nie był widziany na innych poolach. Nie przesądza (może być nowy sybil). |
| CPV ≈ 0.2–0.4 | Kilku buyerów aktywnych cross-pool. Normalne dla aktywnych traderów. |
| CPV ≈ 0.5+ | Większość buyerów rotuje po poolach. Silny sygnał sybil network. |

### Znane ograniczenia
- Fresh wallety (nigdy nie widziane) dają CPV = 0.0 — ale FSC łapie ich przez funding chain.
- Aktywni retail traderzy (degeni snajpujący co 2 minuty nowy launch) dają podwyższone CPV. Dlatego soft signal z umiarkowanym penalty.
- Koszt pamięci: `(Pubkey + Pubkey + u64) × active_signers_per_5min`. Wymaga bounded map.

### Konfiguracja

```toml
max_signer_cross_pool_velocity = 0.50
soft_penalty_high_cpv = 5
cpv_lookback_window_s = 300
```

### Struct

```rust
pub signer_cross_pool_velocity: Option<f64>,
```

### Warunki degradacji
- Rolling state niedostępny / pusty → `None`, degraded_reason += `"CPV_ROLLING_STATE_UNAVAILABLE"`
- `unique_signers_evaluated < 3` → `None`, degraded_reason += `"CPV_INSUFFICIENT_SIGNERS"`

---

## M6. Demand Elasticity Score (DES)

### Co mierzy
Czy rynek **reaguje** na własną dynamikę cenową. W organicznym rynku duży skok ceny powoduje opóźnienie — część buyerów odpada, reszta potrzebuje czasu na decyzję (elastic demand). W cabal pool skrypt kupuje mechanicznie niezależnie od ruchu ceny (inelastic demand).

### Dlaczego to jest potrzebne
To jedyna metryka w stacku która traktuje sekwencję transakcji jako **proces stochastyczny**, nie jako zbiór niezależnych obserwacji. Mierzy warunkową zależność między dynamiką cenową a dynamiką temporalną — coś czego nie łapie żadna inna metryka.

Istniejące metryki mierzą **cechy** transakcji (kto, czym, ile). DES mierzy **dynamikę** rynku (jak rynek reaguje sam na siebie).

### Źródło danych (gRPC)
- `AccountUpdate` na pool state account → `virtual_sol_reserves`, `virtual_token_reserves` po każdej transakcji (już trackowane — w logu: `iwim_snap_virtual_sol_sol`, `iwim_snap_virtual_tokens`)
- `slot` per transakcja z `SubscribeUpdateTransaction`

### Algorytm

**Krok 1: Obliczenie ceny i price impact per buy**

```
price[i] = virtual_sol_reserves[i] / virtual_token_reserves[i]  // po tx[i]
Δprice[j] = (price[j] - price[j-1]) / price[j-1]               // relative price impact tx[j]
```

**Krok 2: Obliczenie inter-buy timing**

```
Δtime[j] = slot[j] - slot[j-1]  // sloty między tx[j-1] a tx[j]
```

**⚠️ UWAGA:** Nie używaj arrival timestamps z gRPC receivera — Geyser emituje post-execution, arrival jitter nie niesie informacji o origin timing (ustalenie z sesji dot. TEJ).

**Dla transakcji w tym samym slocie (Δtime = 0):**
Użyj pozycji w kolejności arrival z gRPC jako sub-slot ordering, rozkładając równomiernie w ramach slotu:
```
if same_slot:
    Δtime = position_in_slot / total_txs_in_slot  // np. 0.0, 0.33, 0.66
```
Nie jest to prawdziwy timing, ale zachowuje monotoniczność sekwencji bez arbitralnej stałej.

**Krok 3: Kendall Tau**

```
pairs = [(Δprice[j], Δtime[j]) for j in 1..N-1]
// Dla N=5 buyów: 4 wartości, C(4,2) = 6 par do porównania

concordant = 0
discordant = 0

for each (i, k) where i < k:
    sign = (Δprice[i] - Δprice[k]) * (Δtime[i] - Δtime[k])
    if sign > 0: concordant += 1
    if sign < 0: discordant += 1

tau = (concordant - discordant) / (concordant + discordant)
```

**Krok 4: Opcjonalny hybrid score (Kendall + Spearman)**

Spearman na N=4 daje inny sygnał (wrażliwy na magnitude, nie tylko rank). Hybrid:

```
spearman = spearman_correlation(Δprice[], Δtime[])
des = 0.5 * tau + 0.5 * spearman
```

Jeśli Spearman jest zbyt kosztowny lub niestabilny na N=4, sam Kendall Tau wystarcza jako v1.

### Interpretacja

| Wartość | Znaczenie |
|---|---|
| DES ≈ +0.5 do +1.0 | Elastyczny popyt. Duży skok ceny → dłuższa przerwa. Rynek reaguje na cenę. Organic. |
| DES ≈ 0.0 | Zerowa elastyczność. Interwały niezależne od ceny. Skrypt kupuje mechanicznie. Cabal. |
| DES ≈ -0.5 do -1.0 | Odwrócona elastyczność. Im większy skok, tym szybciej następny buy. Agresywna akumulacja / front-running bot. |

### Game-theoretic argument (niepodrabialność)
Żeby cabal sfałszował elastyczność, skrypt musiałby **celowo zwalniać po dużych skokach ceny**. Konsekwencje:
- Mniej tokenów kupionych w 8s oknie (inni buyerzy wchodzą przed cablem)
- Gorsze ceny na dalszych odcinkach krzywej
- **Realny koszt: utrata tokenów i gorsza średnia cena wejścia**

Zmusiliśmy cabala do wyboru: trać pieniądze albo trać anonimowość.

### Koszt implementacyjny
Parsowanie `virtual_sol_reserves` z pool AccountUpdate — już istnieje w pipeline. Slot per transakcja — już trackowany. Dodatkowy koszt: 4 odejmowania, 6 porównań znaków, jedno dzielenie. Nanosekundy na hot path.

### Konfiguracja

```toml
min_demand_elasticity_score = 0.15
soft_penalty_inelastic_demand = 6
```

### Struct

```rust
pub demand_elasticity_score: Option<f64>,
```

### Warunki degradacji
- `buy_count < 4` (potrzeba min 3 par Δ, C(3,2)=3 porównania — absolutne minimum) → `None`, degraded_reason += `"DES_INSUFFICIENT_BUYS"`
- Brak danych curve (virtual_sol/tokens unavailable) → `None`, degraded_reason += `"DES_CURVE_DATA_UNAVAILABLE"`

---

## Synergia metryk — Meta-Scoring

### Ortogonalność stacku

Każda metryka atakuje inny wymiar sybil attacku:

| Metryka | Wymiar | Pytanie |
|---|---|---|
| FTDI | infrastruktura | Czym kupują? (toolchain diversity) |
| DBIA | powinowactwo | Czy kupują tym samym co dev? (dev↔buyer affinity) |
| SFD | kapitał | Jaką frakcję portfela wydają? (capital deployment) |
| FSC | funding | Skąd mają SOL? (funding chain) |
| CPV | behawior | Czy pojawiają się na wielu poolach? (cross-pool rotation) |
| DES | dynamika | Czy rynek reaguje na własną cenę? (demand elasticity) |

### Wzorce sprzeczności (cross-metric diagnostics)

| Kombinacja | Interpretacja | Akcja |
|---|---|---|
| high DBIA + low FTDI | Cabal pewniak — wszyscy na skrypcie deva | Wysoki penalty |
| high DBIA + high FTDI | Shared retail bot (dev i buyers na Trojanie) | Ignoruj DBIA |
| low DES + low SFD | Inelastic demand + uniform capital deployment | Najsilniejszy sygnał koordynacji |
| low DES + high SFD | Bot z różnymi budżetami — rzadki, ale możliwy | Umiarkowany penalty |
| high FSC + high CPV | Te same wallety, z jednego źródła, na wielu poolach | Sybil network confirmed |
| high DES + high SFD + low DBIA | Elastic demand, diverse fractions, different infra | Organic — nie penalizuj |

### Koszt ataku — efekt kumulacyjny

Żeby cabal obszedł cały stack jednocześnie, musi:

1. **Użyć różnych botów per wallet** → koszt prowizji (0.5-1% per tx) + operational complexity → obchodzi FTDI
2. **Użyć innego softu niż dev** → oddzielny codebase lub licencja bota → obchodzi DBIA
3. **Zamrozić nadmiar kapitału w walletach** → capital inefficiency → obchodzi SFD
4. **Fundować z różnych źródeł** → multi-hop lub CEX withdrawals → czas + koszty tx → obchodzi FSC
5. **Nie reużywać walletów** → ciągłe generowanie nowej infra → obchodzi CPV
6. **Celowo zwalniać po dużych price impactach** → gorsza średnia cena wejścia → obchodzi DES

Koszt operacyjny rośnie **multiplikatywnie** z każdą warstwą. Jeśli cabal obchodzi wszystkie 6 — zachowuje się jak organic traffic i kosztuje jak organic traffic. Mission accomplished.

---

## Uwagi implementacyjne

### Bounded memory
- FSC i CPV wymagają rolling state (`HashMap` z TTL). Konieczny bounded cap na rozmiar mapy + eviction policy (LRU lub time-based).
- FTDI, DBIA, SFD, DES operują wyłącznie na danych z okna 8s — brak dodatkowych wymagań pamięciowych.

### Hot path performance
- FTDI: O(N) — iteracja po inner_instructions per tx
- DBIA: O(N) — fingerprint per tx + distance per buyer
- SFD: O(N log N) — median wymaga sortowania
- FSC: O(N) — lookup w HashMap
- CPV: O(N) — lookup w HashMap
- DES: O(N²) w teorii (Kendall Tau), ale N ≤ 8 więc C(N,2) ≤ 28 — konstantowy koszt w praktyce

### JSONL export
Wszystkie 6 metryk muszą trafić do `GatekeeperBuyLog` / `gatekeeper_v2_buys.jsonl`:

```rust
pub fee_topology_diversity_index: Option<f64>,
pub dev_buyer_infrastructure_affinity: Option<f64>,
pub spend_fraction_divergence: Option<f64>,
pub funding_source_concentration: Option<f64>,
pub signer_cross_pool_velocity: Option<f64>,
pub demand_elasticity_score: Option<f64>,
```

Serde: `#[serde(skip_serializing_if = "Option::is_none")]`

### FINGERPRINT log extension
```
FINGERPRINT pool=<...> mint=<...> ... ftdi=<...> dbia=<...> sfd=<...> fsc=<...> cpv=<...> des=<...>
```

Formatowanie: `null` jeśli None, 4 miejsca po przecinku dla f64.

---

## Metryki odrzucone podczas sesji (z uzasadnieniem)

| Propozycja | Autor | Powód odrzucenia |
|---|---|---|
| BCE (Bonding Curve Consumption Efficiency) | Claude | Path independence na CPMM — BCE = 1.0 zawsze |
| TEJ (Temporal Entry Jitter) | Claude | Geyser emituje post-execution, arrival jitter nie niesie informacji o origin timing |
| WLCR (Write Lock Contention Ratio) | Claude | Każdy buyer tworzy ATA dla nowego minta — WLCR = 1.0 zawsze w oknie t0 |
| Ψ Entropia dystrybucji | Staß | Max entropia na N=5 = 2.32, próg 4.5 nieosiągalny. Metryka dla N=100+. |
| Ω Trójkąt bermudzki | Staß | Na pump.fun nie ma pre-launch token transfers. Model zakłada pre-minted supply. |
| Λ Entropia czasowa | Staß | Redundantna z istniejącym `timing_entropy`. Gorsza rozdzielczość. |
| Θ Syjamskie bliźniaki | Staß | pump.fun = AMM. Nie ma par buyer↔seller. Graf to gwiazda z poolem. |
| Σ Schizofrenia cenowa | Staß | Jedyna przeżyła review — potencjalny soft signal, ale nie weszła do finalnego stacku. |
| Φ Pętla przyczynowa | Staß | AR(1) + Pearson na N=5 = numerologia. Min 30+ obserwacji. |
| CRP/FBR Fee-Bypass Ratio | Staß | Wymaga hardcoded hashmapy adresów fee walletów = kruche, maintenance-heavy. FTDI robi to samo bez whitelisty. |
| JDA Jito Drain Asymmetry | Staß | W 8s oknie brak sell-side Jito activity. Metryka dla lifecycle analysis (minuty-godziny), nie entry filter. Potencjał w Revolverze, nie Gatekeeperze. |
| CMCS Causal Motif Compression | Jurij G. | 243 możliwe motywy, 5 obserwacji = 2% przestrzeni. Kompresja nie ma czego kompresować. Metryka dla N=50+. |
| LPCS Latent Policy Coherence | Jurij G. | Linear fit 12 parametrów na 5 obserwacjach = underdetermined. PCA na 5×7 matrycy = artefakt małego N, nie sygnał. |
| IECS Impact Elasticity Consistency | Jurij G. | Finite differences Δy/Δx eksplodują przy małych Δx (identyczne buy amounts → Δx≈0 → r_i→∞). Odwrócony sygnał: cabal daje IECS=0 (false organic), organic daje IECS=1 (false cabal). |

---

## Potencjalne rozszerzenia (poza scopem tej specyfikacji)

1. **MetaScorer** — osobny komponent analizujący cross-metric sprzeczności i korelacje (tabela wzorców powyżej). Wymaga kalibracji na datasecie.

2. **RCC (Rank Consistency Coefficient)** — trzeci Kendall Tau: `correlation(ranks(Δtime), ranks(Δsize))`. Ortogonalny do DES i LPCS_reduced. Mierzy czy timing i sizing są spójnie sterowane jedną polityką. Działa na N=5. Kandydat do następnej iteracji.

3. **LPCS_reduced** — Kendall Tau(Δprice, Δsize). "Czy rynek zmniejsza pozycje po dużym ruchu?" Inna oś niż DES (timing). Kandydat do następnej iteracji.

4. **JDA w Revolverze** — Jito Drain Asymmetry jako exit signal w position managerze, nie entry filter. Monitorowanie sell-side Jito activity w rolling window po wejściu w pozycję.
