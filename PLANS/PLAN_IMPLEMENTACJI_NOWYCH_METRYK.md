# PLAN IMPLEMENTACJI NOWYCH METRYK - 2026-04-10

## 0. Cel dokumentu

Ten dokument opisuje **wykonawczy plan wdrożenia 6 nowych metryk** z `NOWE_METRYKI_DO_WDROZENIA.md` w kolejności rekomendowanej po wcześniejszym audycie kosztu i ryzyka:

1. `FTDI` — Fee Topology Diversity Index
2. `DBIA` — Dev-Buyer Infrastructure Affinity
3. `SFD` — Spend Fraction Divergence
4. `DES` — Demand Elasticity Score
5. `CPV` — Signer Cross-Pool Velocity
6. `FSC` — Funding Source Concentration

Celem planu jest wdrożenie tych metryk **bez łamania SSOT, bez obchodzenia istniejących kontraktów architektonicznych i bez regresji w authoritative BUY/REJECT path**.

Plan zakłada dwa poziomy wdrożenia:

1. **wdrożenie obliczeń i pełnej telemetry**,
2. **ostrożną aktywację w policy path** dopiero po wejściu metryk do kanonicznego `MaterializedFeatureSet`.

To jest świadomie plan etapowy. Największym ryzykiem nie jest CPU, tylko przypadkowe wsunięcie nowych sygnałów do niekanonicznej ścieżki i utrata replayability / deterministyczności decyzji.

---

## 1. Nienegocjowalne kontrakty i SSOT

### 1.1. Kontrakty architektoniczne

1. **Authoritative Gatekeeper decisions pozostają oparte wyłącznie o `MaterializedFeatureSet`.**
   - Nie wolno wprowadzić FTDI / DBIA / SFD / DES / CPV / FSC tylko przez `assessment.early_fingerprint` i uznać tego za live filtering.
   - `early_fingerprint` oraz log `FINGERPRINT` pozostają warstwą observability, nie SSOT decyzji.

2. **Nowe metryki muszą być serializowalne i replayowalne.**
   - Jeżeli wpływają na BUY/REJECT, muszą być częścią kanonicznego feature contractu.
   - `None` oznacza brak danych / degradację, a nie syntetyczne `0.0`.

3. **Zmiany transportowe w `TradeEvent` / `PoolTransaction` muszą być addytywne i kompatybilne wstecz.**
   - Nie wolno zmieniać semantyki istniejących pól.
   - Nowe pola muszą mieć `#[serde(default)]` i bezpieczne domyślne zachowanie dla starych fixture'ów i replayów.

4. **CPV i FSC muszą korzystać z bounded rolling state.**
   - TTL, per-key cap, global cap i jawne eviction metrics są obowiązkowe.
   - Żadnych map rosnących bez limitu.

5. **W v1 wszystkie 6 metryk są soft-signalami.**
   - Żadna z nich nie staje się hard-fail kill-switched w pierwszym rolloutcie.
   - Wartości `None` nigdy nie generują kary.

6. **Domyślne konfiguracje muszą być neutralne.**
   - Po merge'u kodu, bez świadomej aktywacji progów / kar, bieżąca decyzja Gatekeepera ma pozostać semantycznie zgodna z dzisiejszym zachowaniem.

### 1.2. Kontrakty semantyczne wynikające ze specyfikacji

1. **FTDI i DBIA nie mogą opierać się na whitelistach botów / platform.**
   - Dopuszczalna jest tylko klasyfikacja topologiczna, nie „adres -> nazwa bota”.

2. **DES nie używa receiver arrival jitter jako proxy timing.**
   - Bazujemy na `slot` + deterministycznym sub-slot ordering.

3. **SFD wymaga realnego udziału wydanej frakcji portfela.**
   - W v1 metryka ma używać dokładnego `pre/post balance` signera, a nie aproksymacji z samego `volume_sol`, jeżeli exact post-balance jest dostępny.

4. **FSC ma jawny wyjątek dla neutralnych funding source typu CEX hot wallet.**
   - Ta lista ma być jawna, wersjonowana i łatwa do audytu.

---

## 2. Stan obecny i luki danych

### 2.1. Co już mamy

W obecnym pipeline są już dostępne lub łatwo materializowalne m.in.:

- `slot`
- `event_ordinal`
- `arrival_ts_ms`
- `cu_price_micro_lamports`
- `compute_unit_limit`
- `inner_ix_count`
- `cpi_depth`
- `ata_create_count`
- `signer_pre_balance_lamports`
- `v_sol_in_bonding_curve`
- `v_tokens_in_bonding_curve`
- `curve_data_known`
- `buy/sell`, signer, pool identity, price trajectory

To oznacza, że **DES** ma najkrótszą drogę do wdrożenia, a **SFD** jest blisko, ale wymaga domknięcia exact post-balance.

### 2.2. Czego dziś brakuje do pełnego wdrożenia

Do bezpiecznej implementacji wszystkich 6 metryk brakuje kilku jawnych danych transportowych:

1. **Dla FTDI / DBIA:**
   - `account_keys_len`
   - `outer_instruction_count`
   - `inner_instruction_group_count`
   - liczba `internal fee transfers`
   - liczba `external fee transfers`
   - jawny filtr / licznik odfiltrowanych transferów WSOL signer↔own ATA

2. **Dla SFD:**
   - `signer_post_balance_lamports`

3. **Dla FSC:**
   - niezależny strumień funding transferów z całego gRPC feedu, nie tylko z tx związanych z analizowanym poolem

Wniosek: **plan nie może zaczynać się od samej matematyki. Najpierw trzeba domknąć transport contract dla surowych danych.**

---

## 3. Docelowa architektura wdrożenia

## 3.1. Kanoniczny feature contract

Zamiast dopisywać 6 nowych pól w różnych miejscach lub wciskać je do `early_fingerprint`, wprowadzamy **dedykowaną kanoniczną grupę features**.

### Proponowany kształt

W `ghost-core` dodać nowy struct, np.:

- `SybilResistanceFeatures`

oraz do `MaterializedFeatureSet` pole:

- `sybil_resistance: SybilResistanceFeatures`

Minimalny zakres danych w tym structcie:

- `fee_topology_diversity_index: Option<f64>`
- `dev_buyer_infrastructure_affinity: Option<f64>`
- `spend_fraction_divergence: Option<f64>`
- `demand_elasticity_score: Option<f64>`
- `signer_cross_pool_velocity: Option<f64>`
- `funding_source_concentration: Option<f64>`
- `degraded_reasons: Vec<String>`
- `buy_sample_count: u64`
- `signer_sample_count: u64`

### Dlaczego osobna grupa features, a nie samo rozszerzenie `TxIntelFeatures`

Bo nowe metryki są mieszane semantycznie:

- część jest **lokalna dla jednego poola** (`FTDI`, `DBIA`, `SFD`, `DES`),
- część wymaga **global rolling state** (`CPV`, `FSC`).

Wepchnięcie wszystkiego do `TxIntelFeatures` zaciera granicę między:

- snapshotem local tx-intel,
- a sygnałami pochodzącymi z dodatkowych indeksów cross-pool / funding.

Osobna grupa jest czytelniejsza, łatwiejsza do audytu i bezpieczniejsza dla przyszłych replayów.

## 3.2. Surowe dane transportowe

W `off-chain/components/seer/src/types.rs` i `ghost-launcher/src/events.rs` należy dodać addytywnie transportowy payload, np.:

- `ToolchainFingerprintInput`
- `FundingTransferObserved`

### `ToolchainFingerprintInput`

Proponowane pola:

- `account_keys_len: Option<u16>`
- `outer_instruction_count: Option<u16>`
- `inner_instruction_group_count: Option<u16>`
- `has_set_compute_unit_limit: Option<bool>`
- `has_set_compute_unit_price: Option<bool>`
- `internal_fee_transfer_count: Option<u8>`
- `external_fee_transfer_count: Option<u8>`
- `filtered_wsol_self_transfer_count: Option<u8>`

Do `TradeEvent` / `PoolTransaction` należy też dodać:

- `signer_post_balance_lamports: Option<u64>`

### Zasada implementacyjna

Transport ma przenosić **surowy materiał wejściowy**, nie gotowe werdykty.

Czyli:

- parser liczy topologię transferów i podstawowe counts,
- launcher liczy metryki FTDI / DBIA / SFD / DES,
- Gatekeeper konsumuje już kanoniczny feature snapshot.

## 3.3. Globalne rolling indexes

Potrzebne są dwa bounded indeksy:

1. `CrossPoolVelocityIndex` — dla `CPV`
2. `FundingSourceIndex` — dla `FSC`

### Wymagania wspólne

Każdy indeks musi mieć:

- TTL (`300 s` jako start)
- per-key cap
- global cap
- licznik evictionów
- licznik lookup hit/miss
- gotowość / warmup state

### Proponowane bezpieczne limity startowe

Te wartości są **punktami startowymi do pomiaru**, a nie dogmatem:

| Indeks | TTL | Per-key cap | Global cap | Źródło zdarzeń |
|---|---:|---:|---:|---|
| `CrossPoolVelocityIndex` | `300 s` | `16` wpisów / signer | `50_000` signerów | `GhostEvent::PoolTransaction` (BUY tylko) |
| `FundingSourceIndex` | `300 s` | `4` wpisy / recipient | `75_000` recipientów | nowy event funding transfer z parsera |

Dla maszyny `x86 / 8 vCPU / 16 GB RAM` to jest rozsądny, konserwatywny punkt wyjścia. Jeżeli rollout pokaże potrzebę wyższych capów, zmiana musi być oparta o pomiar.

---

## 4. Mapa zmian po plikach

| Obszar | Plik / moduł | Zakres zmian |
|---|---|---|
| Canonical feature contract | `ghost-core/src/checkpoint/types.rs` | dodać `sybil_resistance` do `MaterializedFeatureSet` |
| Canonical metric types | `ghost-core/src/tx_intelligence/types.rs` lub nowy moduł | dodać `SybilResistanceFeatures` + enum/stałe degraded reason |
| Parser raw data | `off-chain/components/seer/src/types.rs` | rozszerzyć `TradeEvent` o surowe pola FTDI/DBIA/SFD |
| Parser extraction | `off-chain/components/seer/src/binary_parser.rs` | wyciągnąć topology counts, post-balance, instruction counts |
| Launcher transport bridge | `ghost-launcher/src/components/seer.rs` | mapowanie nowych pól do `PoolTransaction` |
| Launcher event schema | `ghost-launcher/src/events.rs` | rozszerzyć `PoolTransaction`; dodać nowy event funding transfer |
| Local metric calculators | nowy moduł np. `ghost-launcher/src/tx_intelligence/sybil_metrics.rs` | FTDI, DBIA, SFD, DES |
| Cross-pool state | nowy moduł np. `ghost-launcher/src/tx_intelligence/cross_pool_velocity.rs` | CPV index |
| Funding state | nowy moduł np. `ghost-launcher/src/tx_intelligence/funding_source_index.rs` | FSC index |
| Session materialization | `ghost-launcher/src/session/observation.rs` | wypełnić `MaterializedFeatureSet.sybil_resistance` |
| Policy engine | `ghost-launcher/src/components/gatekeeper_policy.rs` | skonsumować `sybil_resistance` wyłącznie przez `feature_snapshot` |
| Assessment/log view | `ghost-launcher/src/components/gatekeeper.rs` | rozszerzyć buy-log mapping i summary |
| Config SSOT | `ghost-brain/src/config/ghost_brain_config.rs` | nowe progi, lookbacki, caps, neutral defaults |
| JSONL schema | `ghost-brain/src/oracle/decision_logger.rs` | nowe pola metryk + degraded reasons |
| Config docs/examples | `ghost-brain/ghost_brain_config.example.toml`, `ghost-brain/GHOST_BRAIN_CONFIG.md` | opis nowych pól |
| Runbook | `docs/RUNBOOK_HOT_PATH_METRICS.md` lub nowy runbook | obserwowalność CPV/FSC i fallbacków |

---

## 5. Strategia wykonawcza

Plan jest celowo rozbity na fazy od najniższego ryzyka kontraktowego do najwyższej złożoności runtime.

## 5.1. Faza 0 — contract-first scaffold

### Cel

Najpierw wprowadzić **szkielet kontraktowy i observability**, bez zmiany produkcyjnej decyzji.

### Kroki

1. Dodać `SybilResistanceFeatures` do `ghost-core`.
2. Dodać `sybil_resistance` do `MaterializedFeatureSet`.
3. Dodać puste / neutralne pola do `GatekeeperBuyLog` i `gatekeeper_v2_buys.jsonl`.
4. Rozszerzyć `FINGERPRINT` log o nowe pola (`ftdi`, `dbia`, `sfd`, `des`, `cpv`, `fsc`) z `null` jeśli `None`.
5. Rozszerzyć `GatekeeperV2Config` o nowe thresholds / penalties / TTL / cap fields, ale z neutralnymi defaultami.
6. Dodać replay test: przy neutralnych defaultach wynik BUY/REJECT dla istniejących fixture'ów nie zmienia się.

### Neutralne defaulty wymagane w tej fazie

- `min_fee_topology_diversity_index = 0.0`
- `max_dev_buyer_infrastructure_affinity = 1.0`
- `min_spend_fraction_divergence = 0.0`
- `min_demand_elasticity_score = -1.0`
- `max_signer_cross_pool_velocity = 1.0`
- `max_funding_source_concentration = 1.0`
- wszystkie nowe penalty / points = `0`

### Exit criteria

- nowe pola pojawiają się w JSONL i serde replay działa,
- brak decyzji drift przy neutralnym configu,
- brak compile/test regressions.

---

## 5.2. Faza 1 — FTDI

### Dlaczego pierwsze

`FTDI` jest względnie tani obliczeniowo, daje wysoką wartość sygnałową i wymusza uporządkowanie parserowego kontraktu dla topologii fee — co od razu przygotowuje grunt pod `DBIA`.

### Zakres implementacji

#### A. Parser / raw transport

W `binary_parser.rs`:

1. przejść po `meta.inner_instructions`,
2. znaleźć wszystkie `SystemProgram::Transfer`,
3. sklasyfikować transfery jako:
   - internal fee transfer,
   - external fee transfer,
   - odfiltrowany transfer signer↔own WSOL ATA,
4. zapisać tylko liczniki, nie pełną listę transferów.

To musi być zrobione **na parserze**, bo późniejsza warstwa nie ma już pełnego execution context potrzebnego do poprawnej klasyfikacji.

#### B. Metric calculator

W nowym module `sybil_metrics.rs`:

1. zebrać buy tx z okna obserwacji,
2. policzyć `topology(tx) = (external_fee_count, internal_fee_count)`,
3. policzyć distinct topology count,
4. wyliczyć `ftdi = unique_topologies / unique_signers_evaluated`.

#### C. Materialization

W `PoolObservationSession::materialize_features(...)` lub równoważnej ścieżce:

- wpisać wynik do `features.sybil_resistance.fee_topology_diversity_index`.

#### D. Degradacja

Stabilne reason codes:

- `FTDI_INSUFFICIENT_BUYS`
- `FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE`

### Policy stance

W pierwszym kroku:

- metryka trafia do kanonicznego feature snapshotu,
- jest logowana,
- ale penalty pozostaje `0`, dopóki replay i shadow bake nie potwierdzą sensownego rozkładu.

### Testy obowiązkowe

1. FTDI nie liczy WSOL self-wrap jako external fee.
2. FTDI rozróżnia `(0,0)`, `(1,0)`, `(2,0)`.
3. Mixed toolchain daje wyższy FTDI niż homogeniczny batch.
4. `buy_count < 3` zwraca `None` + reason.

### Exit criteria

- FTDI obecny w `gatekeeper_v2_buys.jsonl`,
- parser fixture z WSOL nie daje fałszywego external fee,
- neutralny config nie zmienia decyzji.

---

## 5.3. Faza 2 — DBIA

### Dlaczego drugie

`DBIA` dzieli raw dependencies z FTDI, ale ma większe ryzyko false positive, więc ma wejść dopiero po ustabilizowaniu parserowego fingerprintu infrastrukturalnego.

### Zakres implementacji

#### A. Rozszerzenie surowego fingerprintu

Do transportu dodać:

- `account_keys_len`
- `outer_instruction_count`
- `inner_instruction_group_count`
- `has_set_compute_unit_limit`
- `has_set_compute_unit_price`
- reuse `internal_fee_transfer_count` / `external_fee_transfer_count`

#### B. Calculator

1. zbudować fingerprint deva,
2. zbudować fingerprint każdego non-dev buyera,
3. policzyć similarity wg weighted Hamming distance,
4. `dbia = mean(similarity(dev_fp, buyer_fp))`.

### Zasada semantyczna

**DBIA nie może być użyte w policy samotnie.**

Zgodnie ze specyfikacją, wysoki DBIA przy wysokim FTDI może oznaczać shared retail bot, a nie cabal.

Dlatego w v1 policy logic:

- logujemy raw `dbia`,
- ale aktywowany sygnał policy jest dopiero kombinacją:
  - `high_dbia && low_ftdi`

### Degradacja

Stabilne reason codes:

- `DBIA_NO_DEV_BUY`
- `DBIA_INSUFFICIENT_BUYERS`
- `DBIA_RAW_FINGERPRINT_UNAVAILABLE`

### Testy obowiązkowe

1. brak dev buy -> `None` + reason,
2. 1 buyer poza devem -> `None` + reason,
3. identyczne fingerprinty -> wynik blisko `1.0`,
4. różne fingerprinty -> wynik niski,
5. policy nie penalizuje `high_dbia` jeśli `ftdi` jest wysokie.

### Exit criteria

- DBIA serializuje się kanonicznie,
- raw data dla DBIA nie psuje istniejących parse paths,
- istnieje test regresyjny na kombinację `high_dbia + high_ftdi`.

---

## 5.4. Faza 3 — SFD

### Dlaczego trzecie

`SFD` ma wysoką wartość informacyjną, niski koszt CPU i nie wymaga global state. Wymaga jednak domknięcia exact spend fraction po signer post-balance.

### Zakres implementacji

#### A. Transport

Dodać `signer_post_balance_lamports` do `TradeEvent` i `PoolTransaction`.

#### B. Calculator

Dla każdego buy tx:

1. `spend_fraction = (pre_balance - post_balance) / pre_balance`,
2. pominąć rekordy z `pre_balance == 0`,
3. policzyć medianę i MAD.

### Decyzja implementacyjna dla v1

W v1 wdrażamy **standardowy MAD bez ważenia**.

Powód:

- jest stabilny,
- prostszy do audytu,
- w 8s hot path jest wystarczający,
- nie wymaga od razu weighted median implementation i edge-case testowania.

Jeżeli po rolloutcie będzie potrzeba, można dodać równoległy telemetry-only `weighted_sfd_shadow`, ale **nie jest to część tego planu wykonawczego**.

### Degradacja

Stabilne reason codes:

- `SFD_INSUFFICIENT_BUYS`
- `SFD_ZERO_PREBALANCE_SKIPPED`
- `SFD_POSTBALANCE_UNAVAILABLE`

### Policy stance

`low_sfd` może być samodzielnym soft signalem, ale jego penalty pozostaje neutralne do czasu pierwszej walidacji replay/shadow.

### Testy obowiązkowe

1. przykład cabal z niskim MAD,
2. przykład organic z wysokim MAD,
3. `pre_balance == 0` nie wywala kalkulatora,
4. brak post-balance daje `None` + reason.

### Exit criteria

- exact `pre/post` contract działa w parserze i bridge,
- SFD jest wyliczane dla fixture'ów z buy count >= 3,
- brak decyzji drift przy neutralnym configu.

---

## 5.5. Faza 4 — DES

### Dlaczego czwarte

`DES` jest prawie darmowe obliczeniowo i korzysta głównie z danych, które już są w systemie. Wchodzi po SFD, bo razem tworzy silną parę diagnostyczną, ale nie chcemy od razu budować meta-scorera.

### Zakres implementacji

#### A. Ordering contract

Do obliczenia DES używamy kolejności:

1. `slot`,
2. `event_ordinal` jeśli dostępne,
3. fallback: stabilna kolejność bufora sesji dla tx z tego samego slotu.

**Nie używamy receiver jitter jako semantycznego timingu.**

#### B. Calculator

1. z buy-only sequence policzyć `price[i] = v_sol / v_tokens`,
2. policzyć `Δprice`,
3. policzyć `Δtime` na bazie slot/sub-slot ordering,
4. policzyć `Kendall tau` jako `demand_elasticity_score`.

### Decyzja implementacyjna dla v1

W v1 implementujemy **sam Kendall tau**.

Nie dokładamy od razu hybrydy z Spearmanem, bo:

- zwiększa złożoność testów,
- przy małym N nie wnosi wystarczająco dużo, żeby uzasadnić większy blast radius.

### Degradacja

Stabilne reason codes:

- `DES_INSUFFICIENT_BUYS`
- `DES_CURVE_DATA_UNAVAILABLE`
- `DES_SLOT_ORDER_UNAVAILABLE`

### Policy stance

W v1:

- logujemy raw `des`,
- można dodać niezależny sygnał `low_des`,
- ale wzorzec `low_des && low_sfd` pozostaje **na tym etapie tylko diagnozą logową**, nie osobnym boosterem punktów.

To ogranicza ryzyko zbyt agresywnego wejścia w cross-metric logic zanim zobaczymy realne rozkłady.

### Testy obowiązkowe

1. rosnące price impacts + rosnące przerwy dają dodatni DES,
2. price impacts niezależne od timing dają DES około 0,
3. same-slot ordering jest deterministyczny,
4. brak curve data -> `None` + reason.

### Exit criteria

- DES stabilnie liczy się na replay fixtures z curve data,
- brak użycia wall-clock jitter w logice metryki,
- brak decyzji drift przy neutralnym configu.

---

## 5.6. Faza 5 — CPV

### Dlaczego dopiero teraz

`CPV` jest pierwszą metryką wymagającą **global rolling state**. Sama matematyka jest prosta, ale correctness zależy od eviction, TTL i gotowości indeksu.

### Zakres implementacji

#### A. CrossPoolVelocityIndex

Nowy komponent / moduł, np. `cross_pool_velocity.rs`:

- subskrybuje `GhostEvent::PoolTransaction`,
- interesują go tylko `BUY` tx,
- zapisuje dla signera pary `(pool_id, ts_ms)` w bounded state,
- cyklicznie / opportunistycznie prunuje wpisy po TTL.

#### B. Lookup during materialization

Przy materializacji feature snapshotu:

1. wziąć unikalnych buyerów z current pool,
2. dla każdego policzyć liczbę innych pooli w lookback window,
3. `cpv = buyers_seen_elsewhere / unique_signers_evaluated`.

### Degradacja

Stabilne reason codes:

- `CPV_ROLLING_STATE_UNAVAILABLE`
- `CPV_INSUFFICIENT_SIGNERS`

### Wymogi operacyjne

1. indeks musi mieć readiness flagę,
2. indeks ma eksportować metrics:
   - `cpv_index_entries`
   - `cpv_index_evictions_total`
   - `cpv_lookup_hits_total`
   - `cpv_lookup_misses_total`
3. lookup nie może blokować hot path na mutexie o wysokiej kontencji.

### Policy stance

CPV może wejść jako samodzielny soft signal po krótkim telemetry bake, ale dopiero po potwierdzeniu:

- braku memory runaway,
- akceptowalnego lookup latency,
- sensownego baseline distribution na rollout danych.

### Testy obowiązkowe

1. signer aktywny na wielu poolach -> CPV rośnie,
2. signer aktywny tylko lokalnie -> nie podnosi CPV,
3. TTL expiry usuwa stare aktywności,
4. global cap wymusza eviction bez paniców,
5. cold index -> `None` + degraded reason.

### Exit criteria

- bounded index działa pod testem obciążeniowym,
- nie ma unbounded growth,
- CPV pojawia się w JSONL dla warmed-up runtime.

---

## 5.7. Faza 6 — FSC

### Dlaczego ostatnie

`FSC` ma najwyższy koszt wdrożeniowy i najwyższe ryzyko architektoniczne, bo wymaga nowego strumienia danych z **całego** gRPC feedu, nie tylko z pool-trade path.

### Warunek startowy / preflight Fazy 6

Faza 6 **nie może zaczynać się od samej matematyki FSC**. Najpierw trzeba domknąć kanoniczny data-plane dla funding transferów.

Przed rozpoczęciem implementacji obowiązują następujące granice wykonawcze:

1. **Nie wolno wyliczać FSC wyłącznie z pool-trade path.**
   - `PoolTransaction`, `tx_buffer` sesji i lokalna historia buyów nie są wystarczającym źródłem prawdy dla funding provenance.
   - funding source musi pochodzić z niezależnego strumienia transferów z pełnego feedu.

2. **Nie wolno używać RPC/history tracerów jako authoritative hot-path source.**
   - ewentualne ścieżki analityczne / offline tracing mogą służyć diagnostyce,
   - ale nie mogą stać się źródłem prawdy dla produkcyjnego `funding_source_concentration`.

3. **Przed implementacją indeksu trzeba zamrozić lookup semantics.**
   - dla buyera bierzemy **najnowszy kwalifikujący się funding transfer sprzed buy tx** w lookback window,
   - transfery po buy nie mogą nadpisywać funding source dla tego buy,
   - v1 pozostaje **one-hop only** — bez rekursywnego śledzenia grafu finansowania.

4. **Neutral funding sources muszą być modelowane jako jawna klasyfikacja, a nie wspólny bucket koncentracji.**
   - źródło neutralne nie może sztucznie scalać wielu organicznych buyerów w jeden cluster,
   - implementacja powinna rozróżniać co najmniej: `concrete`, `neutral`, `unknown`, `unavailable` lub semantycznie równoważne stany.

### Zakres implementacji

#### A. Funding transfer event

Dodać nowy event bus payload, np.:

- `FundingTransferObserved { from, to, lamports, signature, slot, event_time, arrival_ts_ms }`

Źródło:

- parser / ingest layer widząca pełne `SystemProgram::Transfer` z gRPC,
- tylko transfery powyżej `funding_dust_threshold_lamports`,
- event musi być addytywny i kompatybilny wstecznie na poziomie serde / replay.

Zasada implementacyjna:

- authoritative event powstaje w warstwie widzącej **pełny feed**, nie tylko tx już przypisane do poola,
- parser / ingest odpowiada za identyfikację kwalifikujących się transferów,
- downstream launcher / Gatekeeper nie próbują „odkrywać” funding transferów z niepełnych lokalnych danych.

#### B. FundingSourceIndex

Nowy bounded indeks, współdzielony globalnie analogicznie do `CrossPoolVelocityIndex`:

- key: recipient
- value: ostatnie kwalifikujące się funding transfers w lookback window

Przy lookup dla buyera:

- bierzemy najpóźniejszy sensowny funding source **sprzed buy tx**, 
- lookup musi być wykonywany na współdzielonym indeksie runtime, nie na lokalnym stanie sesji,
- indeks pozostaje bounded: TTL, per-recipient cap, global recipient cap, eviction metrics, hit/miss metrics, readiness / warmup state,
- v1 nie wykonuje multi-hop / recursive trace.

W praktyce indeks powinien być osadzony w infrastrukturze współdzielonej (analogicznie do `SessionManager` + `CrossPoolVelocityIndex`), a materializacja feature snapshotu ma go tylko odpytywać.

#### C. Konfiguracja neutralnych funderów

Lista neutralnych CEX hot walletów ma być:

- jawna,
- wersjonowana,
- ładowana z configu / wspólnego config artifact,
- testowana snapshotem serde.

Dodatkowo:

- neutralny funder nie może sam przez się zwiększać koncentracji tylko dlatego, że wielu buyerów przyszło z tego samego typu CEX hot wallet,
- klasyfikacja neutralności musi być audytowalna i łatwa do zmiany bez ruszania logiki metryki.

#### D. Granica materializacji i policy

`FSC` ma wejść do systemu wyłącznie przez kanoniczną ścieżkę feature snapshotu:

1. pełny feed funding transferów,
2. `FundingTransferObserved`,
3. współdzielony `FundingSourceIndex`,
4. `PoolObservationSession::materialize_features(...)`,
5. `MaterializedFeatureSet.sybil_resistance.funding_source_concentration`,
6. dopiero później policy / soft scoring.

W szczególności:

- materializacja nie wykonuje RPC calli,
- materializacja nie robi history scanów poza współdzielonym bounded indeksem,
- brak streamu lub cold index musi dawać `None` + degraded reason, a nie syntetyczny wynik.

### Degradacja

Stabilne reason codes:

- `FSC_ROLLING_STATE_UNAVAILABLE`
- `FSC_INSUFFICIENT_KNOWN_SOURCES`
- `FSC_FUNDING_STREAM_UNAVAILABLE`

### Policy stance

FSC powinno mieć **najdłuższy telemetry bake** przed aktywacją, bo:

- jest najłatwiejsze do zafałszowania przez błędy klasyfikacji,
- jego skuteczność zależy od jakości całego funding streamu,
- każda pomyłka w neutralizacji CEX hot wallets może dać false positive cluster.

Pierwszy merge Fazy 6 powinien dowieźć przede wszystkim:

- authoritative funding-transfer transport,
- bounded `FundingSourceIndex`,
- pełną telemetry / degraded reasons,
- neutralne domyślne policy (`soft_penalty_high_fsc = 0`).

Aktywacja realnej kary dla `FSC` może nastąpić dopiero po potwierdzeniu, że stream jest kompletny operacyjnie i że neutral-funder classification nie generuje sztucznych klastrów.

### Testy obowiązkowe

1. kilku buyerów z jednego fundera -> wysoki FSC,
2. funding z różnych źródeł -> niski FSC,
3. kilku buyerów z Binance hot wallet -> brak sztucznego zawyżenia,
4. TTL expiry i cap eviction działają,
5. brak funding streamu -> `None` + reason,
6. lookup wybiera najpóźniejszy kwalifikujący się transfer **sprzed buy**,
7. transfer po buy nie nadpisuje funding source użytego do FSC,
8. cold index / brak readiness nie generuje syntetycznego wyniku,
9. materializacja FSC nie zależy od RPC fallbacku.

### Exit criteria

- funding stream jest stabilnie emitowany,
- funding stream pochodzi z authoritative full-feed path, nie z pool-local heurystyki,
- FSC nie wprowadza runaway memory,
- istnieje współdzielony bounded `FundingSourceIndex` z TTL, capami, eviction metrics i readiness,
- lookup semantics dla „latest eligible pre-buy funding source” są potwierdzone testami,
- neutral funder list działa zgodnie z fixture'ami,
- materializacja zwraca `None` przy braku streamu / cold index zamiast syntetycznego wyniku.

---

## 6. Integracja z Gatekeeper policy

## 6.1. Zasada główna

Nowe metryki **nie** mają być wprowadzane do decyzji przez `assessment.early_fingerprint`.

Jedyna poprawna ścieżka jest taka:

1. parser / raw transport,
2. `PoolTransaction` / session aggregation,
3. `MaterializedFeatureSet.sybil_resistance`,
4. `build_assessment_from_features(...)`,
5. `compute_soft_signals(...)` lub nowy dedykowany soft-signal pass.

## 6.2. Rekomendowana implementacja scoringu

Aby nie rozwalić istniejącego legacy soft scoringu, wdrożyć **oddzielny sybil soft bucket**.

### Proponowany model

1. zachować istniejące `SoftSignals` i obecne grupy bez zmiany semantyki,
2. dodać nowy struct, np. `SybilSoftSignals`, zawierający:
   - `low_ftdi`
   - `high_dbia`
   - `low_sfd`
   - `low_des`
   - `high_cpv`
   - `high_fsc`
   - `high_dbia_low_ftdi_combo`
3. policzyć osobno `legacy_soft_points`,
4. policzyć osobno `sybil_soft_points`,
5. finalnie użyć:
   - `total_soft_points = legacy_soft_points + sybil_soft_points`

### Dlaczego tak

Bo obecny `SoftSignals` jest grupowany w stare kategorie (`timing`, `manipulation`, `diversity`, `ecosystem`). Wciskanie 6 nowych metryk do tych grup zaciera semantykę i robi niepotrzebny bałagan.

## 6.3. Zasady aktywacji policy dla v1

1. **FTDI** — samodzielny soft flag, ale nie hard fail.
2. **DBIA** — policy używa dopiero kombinacji `high_dbia && low_ftdi`.
3. **SFD** — samodzielny soft flag.
4. **DES** — samodzielny soft flag.
5. **CPV** — samodzielny soft flag po warmupie indeksu.
6. **FSC** — samodzielny soft flag dopiero po najdłuższym bake.

Nie wprowadzamy w tym planie pełnego MetaScorera. Najpierw bazowe metryki muszą być poprawnie policzone i ustabilizowane.

---

## 7. Konfiguracja

## 7.1. Pola do dodania do `GatekeeperV2Config`

### Progi metryk

- `min_fee_topology_diversity_index: f64`
- `max_dev_buyer_infrastructure_affinity: f64`
- `min_spend_fraction_divergence: f64`
- `min_demand_elasticity_score: f64`
- `max_signer_cross_pool_velocity: f64`
- `max_funding_source_concentration: f64`

### Scoring / penalties

- `soft_penalty_low_ftdi: u8`
- `soft_penalty_high_dbia: u8`
- `soft_penalty_low_sfd: u8`
- `soft_penalty_inelastic_demand: u8`
- `soft_penalty_high_cpv: u8`
- `soft_penalty_high_fsc: u8`
- `soft_penalty_high_dbia_low_ftdi_combo: u8`

### Rolling-state params

- `cpv_lookback_window_s: u64`
- `funding_lookback_window_s: u64`
- `funding_dust_threshold_lamports: u64`
- `cpv_per_signer_cap: usize`
- `cpv_global_signer_cap: usize`
- `fsc_per_recipient_cap: usize`
- `fsc_global_recipient_cap: usize`

### Neutral funders

- `neutral_funding_sources: Vec<String>`

## 7.2. Default policy

Domyślne wartości po merge'u mają być neutralne — tak, aby wdrożony kod najpierw zbierał dane i nie zmieniał decyzji bez jawnej aktywacji configiem.

---

## 8. Observability i eksport

## 8.1. JSONL

W `GatekeeperBuyLog` dodać pola:

- `fee_topology_diversity_index`
- `dev_buyer_infrastructure_affinity`
- `spend_fraction_divergence`
- `demand_elasticity_score`
- `signer_cross_pool_velocity`
- `funding_source_concentration`
- `sybil_metric_degraded_reasons`

Zasada serde:

- `Option<f64>` → `skip_serializing_if = "Option::is_none"`
- degraded reasons serializowane tylko jeśli niepuste

## 8.2. FINGERPRINT log

Log `FINGERPRINT` ma mirrorować wynik, ale nie być źródłem prawdy.

Docelowy format przykładowy:

- `ftdi=<...>`
- `dbia=<...>`
- `sfd=<...>`
- `des=<...>`
- `cpv=<...>`
- `fsc=<...>`
- `sybil_degraded=<...>`

## 8.3. Runtime metrics

Dla CPV / FSC dodać runtime counters/gauges:

- current entries
- per-key overflows
- global evictions
- lookup hit/miss
- warmup ready state
- prune duration

Bez tego nie da się bezpiecznie ocenić, czy 8 vCPU / 16 GB nadal trzyma się w ryzach podczas rollout.

---

## 9. Test plan

## 9.1. Unit tests

Obowiązkowe osobne testy jednostkowe dla:

- FTDI topology classifier
- DBIA weighted similarity
- SFD MAD
- DES Kendall tau
- CPV TTL/cap lookup
- FSC funding source lookup + neutral funder handling

## 9.2. Replay / integration tests

1. Fixture replay z istniejących pooli.
2. Replay z neutralnym configiem -> **zero decision drift**.
3. Replay z metrykami aktywnymi -> expected drift opisany snapshotem.
4. Serde snapshot dla `MaterializedFeatureSet` z nowym `sybil_resistance`.
5. Serde snapshot dla `GatekeeperBuyLog`.

## 9.3. Performance / bounded-state tests

Dla CPV i FSC dodać testy, które wprost sprawdzają:

- TTL prune,
- cap eviction,
- brak unbounded growth,
- brak paniców przy dużej liczbie signerów / recipientów.

## 9.4. Parser regression tests

Dla FTDI / DBIA / SFD potrzebne są parser fixtures obejmujące:

- WSOL wrap/unwrap,
- różne topologie fee,
- buy tx z compute budget instructions,
- signer pre/post balance extraction.

---

## 10. Plan rolloutu

## 10.1. Kolejność aktywacji

1. Merge fazy 0 z neutralnym configiem.
2. Aktywować telemetry-only dla FTDI.
3. Aktywować telemetry-only dla DBIA.
4. Aktywować telemetry-only dla SFD.
5. Aktywować telemetry-only dla DES.
6. Aktywować telemetry-only dla CPV.
7. Aktywować telemetry-only dla FSC.
8. Po zebraniu rollout danych aktywować policy kolejno:
   - FTDI
   - DBIA tylko jako `high_dbia_low_ftdi_combo`
   - SFD
   - DES
   - CPV
   - FSC

## 10.2. Go / no-go przed aktywacją każdego kroku policy

Dla każdej metryki obowiązuje ten sam gate:

1. brak compile/test regressions,
2. brak runaway memory,
3. poprawny JSONL export,
4. brak parser false positives na fixture'ach,
5. znany baseline distribution z rollout danych,
6. jawnie ustawione progi i penalties w configu.

---

## 11. Definition of Done

Plan uznajemy za wykonany dopiero, gdy spełnione są łącznie wszystkie poniższe warunki:

1. wszystkie 6 metryk są liczone i obecne w `MaterializedFeatureSet.sybil_resistance`,
2. wszystkie 6 metryk trafiają do `gatekeeper_v2_buys.jsonl`,
3. wszystkie 6 metryk mają stabilne degraded reasons,
4. FTDI / DBIA / SFD / DES działają bez global state,
5. CPV / FSC działają na bounded rolling indexes z TTL i eviction metrics,
6. neutralny config nie zmienia istniejących decyzji,
7. aktywowany config zmienia decyzje tylko przez kanoniczny policy path,
8. istnieją testy parserowe, jednostkowe, replay i bounded-state,
9. runbook i config docs są zaktualizowane,
10. wdrożenie nie polega na `early_fingerprint` jako authoritative source.

---

## 12. Rekomendacja końcowa

Najbezpieczniejsza ścieżka dla tego repo to:

1. **najpierw dodać kanoniczny kontrakt feature + observability**,
2. **następnie dowieźć lokalne metryki (`FTDI`, `DBIA`, `SFD`, `DES`)**,
3. **potem bounded cross-pool index (`CPV`)**,
4. **na końcu funding index (`FSC`)**,
5. **policy aktywować dopiero po telemetry bake i replay diff**.

To zachowuje SSOT, nie rozwala obecnej architektury i jednocześnie daje ścieżkę do pełnego wdrożenia wszystkich 6 metryk w sposób produkcyjnie sensowny, a nie „na skróty”.
