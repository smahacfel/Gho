## GHOST BRAIN CONFIG MANUAL

Poniżej opisano sposób w jaki należy podejść do konfigurowania/nastrajania mózgu Ghosta. Prawidłowe zdefiniowanie wartości dla większości parametrów przekładają się ostatecznie na strategię, jaką przyjmie Ghost.
Może to być np. strategia wybrednego bydlaka, czy wręc przeciwnie - rzucającego się na wszystko, szybkiego pojeba. 


###  KATEGORIA 1: "Parametry krytycznie istotne" (Stroić w pierwszej kolejności)

To są parametry, które bezpośrednio decydują o: **KUPIĆ czy OLAĆ**. Zmiana o 0.1 zmienia wynik finansowy.

#### 1\. `[qass] -> score_threshold_viral` (Default: 0.85)

  * **Co to robi:** To jest ostateczna poprzeczka. Jeśli Oracle wyliczy wynik 0.84, a próg to 0.85 -\> **SKIP**.
  * **Jak stroić:**
      * **Zbyt ostrożny (missed opportunities):** Zmniejsz do **0.80**.
      * **Kupuje śmieci (false positives):** Zwiększ do **0.90**.
      * *To jest najważniejsza liczba w całym pliku.*

#### 2\. `[sobp] -> hyper_pump_threshold` (Default: 3.0)

  * **Co to robi:** Definiuje, co uznajemy za "Moonshot". SOBP mierzy ciśnienie zakupowe slot po slocie. Wartość 3.0 oznacza 3-krotny wzrost presji.
  * **Wartość:** Dla strategii Predator, zależy nam na wykrywaniu nagłego napływu kapitału.
      * Jeśli ustawisz za wysoko (np. 5.0), wejdziesz za późno.
      * Jeśli za nisko (np. 1.5), kupisz każdy mały skok, który zaraz zgaśnie.

#### 3\. `[sobp] -> human_weight_multiplier` (Default: 2.0)

  * **Co to robi:** Mówi botowi: *"Jeden dolar od człowieka jest warty dwa razy więcej niż dolar od bota"*.
  * **Dlaczego to ważne:** Boty (Snipry) wchodzą i wychodzą (dumpują). Ludzie (Retail) wchodzą i trzymają (HODL), tworząc podłogę ceny.
  * **Strategia:** Zostaw 2.0 lub zwiększ do **2.5**, jeśli chcesz polować na "Organic Gems".

-----

### 🟡 KATEGORIA 2: "Bezpieczniki" 

Te parametry chronią Cię przed Rug Pullami i innym gównem jakich w sieci jest w chuj. Ich błędna konfiguracja, w dużej części przypadków może oznaczaczać stratę całego wkładu.

#### 1\. `[iwim] -> iapp_rug_threshold` (Default: 2)

  * **Co to robi:** Jeśli Dev utworzył w jednej transakcji (lub bloku) **2 lub więcej** kont tokenowych, system uznaje to za **RUG PULL na 97%**.
  * **Zalecenie:** **NIE DOTYKAĆ.** To jest "Death Star Setting". Jeśli Dev robi 2 konta, to znaczy, że jedno jest dla niego (ukryte), a drugie oficjalne. To zawsze scam.

#### 2\. `[iwim] -> min_iapp_rug_score` (Default: 0.95)

  * **Co to robi:** Jak mocno ten sygnał wpływa na negatywną ocenę. 0.95 oznacza "Prawie pewna śmierć".
  * **Zalecenie:** Zostawić.

-----

### KATEGORIA 3: "Detektory aktorów" (MPCF & SSMI)

To jest silnik do rozróżniania kto bot, a kto człowiek. Działa na poziomie mikrosekund i bajtów.

#### 1\. `[mpcf] -> bot_entropy_threshold` (3.5) & `human_entropy_threshold` (5.5)

  * **Jak to działa:** Boty są nudne (powtarzalne instrukcje = niska entropia). Ludzie są chaotyczni (klikanie myszką = wysoka entropia).
  * **Strojenie:** Jeśli widzisz w logach, że system klasyfikuje znane boty jako ludzi -\> **Zmniejsz** próg bota (np. na 3.0). Jeśli klasyfikuje ludzi jako boty -\> **Zmniejsz** próg człowieka (np. na 5.0).

#### 2\. `[ssmi] -> viral_min_tx_count` (Default: 6)

  * **UWAGA DLA SNIPERA:** W Twoim trybie (`min_txs = 0` / Sniper Mode), ten parametr może być mylący dla oceny początkowej ("Initial Score"), bo będziesz miał 0 lub 1 tx.
  * **Wpływ:** Ten parametr ma znaczenie dla **Followup Score** (ocena po 1s, 5s). Jeśli po 5 sekundach nie ma 6 transakcji -\> Entuzjazm opada.
  * **Zalecenie:** Dla strategii "Predator" (wejście w 2-4 sekundzie), wartość **6** jest OK. Oznacza to, że oczekujemy, iż tłum dołączy natychmiast.

-----

### ⚪ KATEGORIA 4: "Lepiej nie ruszaj"

Parametry matematyczne niskiego poziomu. Zmiana ich bez doktoratu z matematyki, albo finału w Kangurze, zepsuje algorytm.

  * `[qofsv] -> epsilon`, `target_construction_time_us`
  * `[frb] -> min_amplitude_threshold`
  * `[resonance] -> bot_threshold_cv` (Współczynnik zmienności dla rezonansu).

**Zalecenie:** Zostaw je na domyślnych wartościach. One zostały dobrane eksperymentalnie.

-----

### 🔵 KATEGORIA 5: "Early Stage Detection vs Gatekeeper"

**Q: What's the difference between `min_tx_count_for_scoring` and Early Stage threshold?**

**A: Two-phase filtering system:**

1. **Gatekeeper (Hard Filter)**
   - Config: `[oracle.sampling_loop] min_tx_count_for_scoring = 15`
   - Location: `oracle_runtime.rs`
   - Action: **REJECT** pools with `< 15 TX`
   - Purpose: Eliminate dead pools before wasting scoring resources

2. **Early Stage Detection (Adaptive Analysis)**
   - Threshold: Gatekeeper × 1.5 = **22 TX** (calculated in orchestrator)
   - Location: `orchestrator.rs`
   - Action: **SKIP TREND METRICS** for pools with 15-22 TX
   - Purpose: Prevent false negatives from insufficient data

**Example Timeline:**
```
Pool with 18 TX arrives at orchestrator:
✅ Passed Gatekeeper (18 ≥ 15)
✅ Enters Early Stage Mode (18 < 22)
   → Runs: LIGMA, IWIM, Chaos, MESA
   → Skips: SCR, ULVF, POVC (need more history)
   
Pool with 25 TX arrives at orchestrator:
✅ Passed Gatekeeper (25 ≥ 15)
✅ Enters Full Analysis (25 ≥ 22)
   → Runs: ALL metrics including trend-based
```

**Tuning Recommendations:**
- **Conservative** (prefer quality): Keep `min_tx_count_for_scoring = 15`
- **Aggressive** (more opportunities): Lower to `min_tx_count_for_scoring = 10`
  - Early Stage will activate for 10-15 TX pools
  - Full Analysis starts at 15 TX
