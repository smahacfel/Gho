# ACE Core — One-Day Kill Test V3 po PR #86

## Szybka falsyfikacja SELECTED vs REST na aktywnej granicy PR1E

**Status:** READY FOR IMPLEMENTATION / QUICK-FALSIFICATION-ONLY / PR2 WSTRZYMANY
**Repozytorium:** `smahacfel/Gho`
**Baseline:** `origin/main = 43057b296663129ca9b4f572e793474830a5452c`
**Źródło baseline:** merge PR #86 — `PR1E: activate ingest-state runtime authority and qualify PR1 end-to-end`
**Badana akcja:** wyłącznie diagnostyczne `ENTER_CONTINUATION` kontra `NO_ENTRY`
**Notional proxy:** całkowity wallet-debit cap `150_000_000` lamportów
**Czas pierwszego rozstrzygnięcia:** jeden capture 24 h; drugi dzień tylko przy negatywnym lub mieszanym Dniu 1
**Runtime authority:** bez zmian
**PR2 ingest–state–quote:** poza zakresem i nadal wstrzymany

---

## 0. Decyzja wykonawcza

Po PR #86 nie przechodzimy do PR2. Nie budujemy transaction-local exact trajectory, anchored quote authority, complete execution reconstruction ani nowego decision plane.

Wykonujemy najmniejszy wiarygodny test, który może szybko zabić hipotezę ACE:

```text
aktywny PR1E canonical runtime
→ istniejący full-universe observe-only tape
→ jeden cutoff na birth
→ dokładnie pięć prostych cech
→ zamrożony score po pierwszych 250 rows
→ SELECTED albo REST
→ ten sam sustained economic proxy dla każdego evaluable birth
→ mean(SELECTED) - mean(REST)
→ median(SELECTED) - median(REST)
```

Nie powstają:

- PR A ani PR B ze starego planu ACE;
- zmiany `MaterializedFeatureSet`;
- model ML;
- Ridge;
- bootstrap;
- failure semantics;
- Position Manager lifecycle;
- entry intent;
- live inference;
- sequential portfolio replay;
- transaction-local exact anchors z PR2;
- system promocji.

Dopuszczalny jest jeden mały, izolowany change set potrzebny wyłącznie do capture i uruchomienia probe.

---

## 1. Zweryfikowany stan repozytorium po PR #86

### 1.1. Baseline

Aktualny `origin/main`:

```text
43057b296663129ca9b4f572e793474830a5452c
```

PR #86 jest zmergowany. Jego aktywny przepływ to:

```text
aligned primary raw wrapper
→ PumpObservationLedgerV1
→ private CanonicalRuntimePermitV1
→ Event Bus / Oracle / session
→ actual downstream apply acknowledgement
→ CandidateIntegrity generation/CAS gate
→ MFS / evaluation / guarded submit
```

PR1E nie zmienił:

- `PumpQuoteV1`;
- quote math;
- program fees;
- transaction costs;
- entry sizing;
- strategii;
- MFS schema;
- Position Managera;
- route authorization;
- PR2 transaction-local anchors.

Oznacza to, że obecna granica ingest/state jest wystarczająco dobra do szybkiego testu sygnału, ale nie daje jeszcze pełnego exact executable replay.

### 1.2. Universe po PR1E

Po PR1E `NewPoolDetected` bez poprawnego prywatnego permitu jest odrzucany przed aktywnym runtime. Durable birth row jest emitowany dopiero po walidacji permitu.

Dlatego universe testu definiujemy jako:

```text
wszystkie PR1E-canonical-permitted Pump/SOL births
w jednym authority epoch i jednym ciągłym capture scope
```

Nie są częścią eligible universe:

- parsed NLN-only births;
- secondary-witness-only observations;
- duplicate primary observations;
- wrappery bez permitu;
- obserwacje technicznie zablokowane przed uzyskaniem canonical authority.

To nie jest strategiczne filtrowanie. Są to obserwacje, na których aktywny Ghost po PR1E nie ma prawa rozpocząć oceny.

### 1.3. Full-universe trade tape pozostaje dostępny

W `oracle_runtime.rs` birth evidence jest emitowany po permit validation. Trade evidence jest emitowany przed `rejected_pools` filtering, więc tape nie ogranicza się do obecnych Gatekeeper BUY/PASS.

Istniejący `FullUniverseReserveJoiner` jest evidence-only. Uzupełnia trade o real reserves wyłącznie przy dokładnym joinie:

```text
(slot, bonding_curve, virtual_sol_reserves, virtual_token_reserves)
```

Jeżeli stan jest brakujący, konfliktowy lub przekroczy TTL, row zostaje zapisany bez zgadywania real reserves. Nie ma fallbacku do latest mutable state ani mark price.

### 1.4. PR2 nadal nie istnieje

`PumpEconomicCertificationStatusV1` nadal nie posiada transaction-local exact trajectory dla badanego tape. One-day probe musi więc uczciwie pozostać:

```text
observed_path_non_propagated_sustained_proxy
```

Nie wolno nazywać wyniku executable EV.

---

## 2. Krytyczna korekta względem V2: obecnego amount field nie wolno użyć

### 2.1. Znaleziony rozjazd

Aktualny adapter:

```text
ghost-launcher/src/components/seer.rs:6628-6667
```

ustawia:

```text
BUY  → PoolTransaction.sol_amount_lamports = TradeEvent.max_sol_cost
SELL → PoolTransaction.sol_amount_lamports = TradeEvent.min_sol_output
```

Są to instruction cap/floor, a nie udowodniony actual curve quote.

Następnie full-universe payload kopiuje ten field do:

```text
effective_curve_quote_lamports
```

przy successful non-synthetic trade.

W konsekwencji obecne:

```text
sol_amount_lamports
effective_curve_quote_lamports
volume_sol
dev_buy_lamports
```

nie mogą być podstawą x1, x3, x4 ani x5 w tym teście. Naprawienie ich pełnej semantyki należy do PR2 i nie będzie wykonywane teraz.

### 2.2. Minimalna zamiana źródła przepływu

`TradeEvent` oraz wewnętrzny `PoolTransaction` już zachowują:

```text
signer_pre_balance_lamports
signer_post_balance_lamports
```

One-day capture musi wyłącznie dopisać te dwa istniejące pola do durable `PoolTransactionPayload`.

Dla successful BUY definiujemy:

```text
observed_buy_wallet_debit_lamports =
    signer_pre_balance_lamports
    - signer_post_balance_lamports
```

Warunki:

```text
pre i post są dostępne
pre > post
BUY jest successful
row nie jest synthetic
wallet identity jest znane
canonical order jest kompletny
```

Jest to celowo nazwane:

```text
observed wallet-debit proxy
```

Nie jest to:

- Pump curve quote;
- exact program debit;
- transaction-local settlement;
- PR2 economics.

Proxy zawiera także koszty transakcyjne poniesione przez wallet. Na etapie szybkiej selekcji jest to dopuszczalne, ponieważ identyczna definicja obowiązuje dla SELECTED i REST, a wynik nie jest przedstawiany jako EV.

Nie ma fallbacku do `sol_amount_lamports`, `effective_curve_quote_lamports`, `volume_sol`, ceny ani token amount.

---

## 3. Minimalny change set

### 3.1. Dwa pola w istniejącym durable tape

#### `ghost-brain/src/events/schema.rs`

Typ:

```text
PoolTransactionPayload
```

Dodać:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub signer_pre_balance_lamports: Option<u64>,

#[serde(default, skip_serializing_if = "Option::is_none")]
pub signer_post_balance_lamports: Option<u64>,
```

Nie dodawać derived score, candidate flag ani outcome.

#### `ghost-launcher/src/oracle_runtime.rs`

Funkcja:

```text
pool_transaction_evidence_payload(...)
```

Przenieść bez transformacji:

```text
tx.signer_pre_balance_lamports
tx.signer_post_balance_lamports
```

### 3.2. Freeze typed quote authority do późniejszego offline użycia

Aktualny `RugRealityCaptureRunManifestV1` zapisuje jedynie metadata fee authority, ale nie zapisuje samych route-specific `ProgramFeeSchedule` potrzebnych do quote’owania historycznych slotów.

Ponowne pobranie fee authority dopiero po 24 h jest niewystarczające: nowo pobrany schedule może mieć `effective_slot` późniejszy niż początki tape i registry poprawnie odrzuci historyczny quote.

#### `ghost-launcher/src/rug_reality_capture.rs`

Rozszerzyć istniejący manifest o jeden field:

```rust
pub pump_quote_authority: RugScalpPumpQuoteAuthorityV1,
```

Typ jest już serializowalny i zawiera:

- route-specific schedules dla `BuyV2` i `LegacySell`;
- slot-resolved evidence;
- entry transaction costs;
- exit transaction costs.

#### `ghost-launcher/src/main.rs`

Przy materializacji istniejącej reality-capture authority zachować zwrócony:

```text
RugScalpPumpQuoteAuthorityV1
```

i zapisać go w tym samym istniejącym run manifest.

Nie powstaje osobny registry, receipt framework ani fee service.

### 3.3. Jeden offline Rust probe

Dodać:

```text
ghost-launcher/src/ace_core_one_day_probe.rs
ghost-launcher/src/bin/ace_core_one_day_probe.rs
```

oraz jeden eksport:

```text
ghost-launcher/src/lib.rs
```

Library module:

- czyta event JSONL;
- czyta reality-capture manifest;
- materializuje istniejący `RugScalpPumpQuoteContractV1` z manifestu;
- buduje rows;
- zapisuje calibration file, candidate rows i summary.

Thin binary odpowiada tylko za CLI.

Nie powstaje runtime reducer, observer task ani event-bus subscriber.

### 3.4. Jeden nowy config capture

Dodać:

```text
configs/rollout/ace-core-one-day-probe-r1.toml
```

Bazować na istniejącym full-universe reality capture, ale obowiązkowo:

```toml
[trigger]
enabled = false
max_concurrent_positions = 1

[execution]
execution_mode = "shadow"

[p37_shadow_probe]
enabled = false

[rug_scalp_v2]
enabled = false

[rug_reality_capture]
enabled = true

[execution.events]
enable_optional_events = true
enable_aem_ticks = false
```

`PoolTransaction` jest w EventWriter oznaczony jako event opcjonalny. Przy `enable_optional_events = false` powstałyby birth rows bez tape transakcji, co czyni test bezwartościowym.

`trigger.max_position_size_sol` pozostaje nieistotny, ponieważ Trigger jest wyłączony. Notional `0,15 SOL` istnieje wyłącznie w offline probe.

Wszystkie ścieżki, `run_id`, manifest path i output directory muszą być nowe. Nie wolno nadpisywać R6 ani wcześniejszych capture.

---

## 4. Input i strict join

Probe przyjmuje:

```text
--events-dir <execution.events.output_dir>
--manifest <rug_reality_capture.manifest_path>
--output-dir <ace probe output>
--day-id day1|day2
--calibration <opcjonalnie: calibration_v1.json dla day2>
```

Join birth ↔ trades:

```text
(base_mint, bonding_curve)
```

Zasady:

1. birth musi być `NewPoolDetected` z Pump programem i WSOL quote;
2. przy powtórzonym birth key wygrywa najwcześniejszy canonical birth; kolejne są reason-coded jako duplicate birth evidence;
3. trade musi mieć zgodny mint i bonding curve;
4. trade dedupe key:

```text
signature
+ slot
+ tx_index
+ outer_instruction_index
+ inner_group_index
+ event_ordinal
```

5. sama signature nigdy nie deduplikuje kilku legalnych mutacji w jednej transakcji;
6. brak pełnego order key w feature window daje `NON_EVALUABLE_FEATURES`;
7. brak pełnych czterech reserves w entry/outcome state daje `NON_EVALUABLE_RESERVES` dla danego birth.

---

## 5. Universe i capture validity

### 5.1. Eligible denominator

Do denominatora trafia każdy PR1E-canonical-permitted Pump/SOL birth zapisany w runie.

Każdy birth otrzymuje dokładnie jeden status:

```text
CALIBRATION_EXCLUDED
EVALUABLE_SELECTED
EVALUABLE_REST
NON_EVALUABLE_FEATURES
NON_EVALUABLE_RESERVES
NON_EVALUABLE_CAPACITY
NON_EVALUABLE_SUSTAIN_COVERAGE
INVALID_CAPTURE
```

### 5.2. Run-level invalidation

Cały dzień jest `INVALID_CAPTURE`, jeżeli wystąpi którekolwiek z:

- PR1E global candidate admission zostaje zamknięty;
- `pr1_runtime_bypass_attempt_total > 0`;
- primary local coverage gap przecina capture;
- EventWriter nie domknął plików albo JSONL jest uszkodzony;
- manifest authority/config/run ID nie zgadza się z tape;
- capture został uruchomiony z `enable_optional_events = false`;
- launcher zatrzymał się przed zadeklarowanym końcem dnia bez kontrolowanego flushu.

Nie dodajemy nowego health frameworka. Korzystamy z istniejących PR1E logs/counters i istniejących output files.

---

## 6. Cutoff-safety

Dla każdego birth:

```text
cutoff_ts_ms = birth_ts_ms + 11_111
```

Do cech wchodzą wyłącznie successful BUY rows:

```text
event_ts_ms <= cutoff_ts_ms
```

Post-cutoff state jest outcome-only.

Entry state dla proxy:

```text
ostatni complete=false full-reserve state
nie późniejszy niż cutoff
```

Dodatkowy warunek:

```text
cutoff_ts_ms - entry_state.event_ts_ms <= 1_000 ms
```

Brak świeżego entry state daje:

```text
NON_EVALUABLE_RESERVES
reason = entry_state_missing_or_stale
```

Żaden późniejszy state nie może zostać użyty do rekonstrukcji cutoff features ani entry state.

---

## 7. Dokładnie pięć cech po korekcie amount source

Wszystkie amount-based cechy korzystają wyłącznie z:

```text
observed_buy_wallet_debit_lamports
```

### x1 — creator buy wallet-debit share

W pełnym oknie od birth do cutoff:

```text
creator_buy_debit =
    suma wallet debit successful BUY rows,
    dla których signer == birth.creator

total_buy_debit =
    suma wallet debit wszystkich successful BUY rows

x1 = creator_buy_debit / total_buy_debit
```

Niżej = lepiej.

To jest operacyjny odpowiednik intencji dawnego `dev_volume_ratio`, ale nie używa jego obecnie skażonej amount surface.

### x2 — shrinkowany log-ratio intensywności nowych kupujących

```text
N_short = liczba first buys nowych walletów w [cutoff - 2 s, cutoff]
N_long  = liczba first buys nowych walletów w [cutoff - 8 s, cutoff]

lambda_short = (N_short + 1) / 3
lambda_long  = (N_long  + 1) / 9

x2 = ln(lambda_short / lambda_long)
```

Wyżej = lepiej.

### x3 — first-buy wallet-debit flow

```text
x3_lamports =
    suma observed_buy_wallet_debit_lamports
    pierwszego BUY każdego walleta
    w [cutoff - 8 s, cutoff]
```

Wyżej = lepiej.

### x4 — trend wielkości first buys

First buys są sortowane po canonical order.

Wymagane minimum czterech first buys.

```text
early = mediana pierwszej połowy wallet debit
late  = mediana drugiej połowy wallet debit

x4 = ln((late + 1) / (early + 1))
```

Wyżej = lepiej.

### x5 — HHI first-buy wallet-debit flow

```text
x5 = Σ(first_buy_debit_i / x3_lamports)^2
```

Niżej = lepiej.

### Feature non-evaluable

Cały feature row jest non-evaluable, gdy:

- creator identity jest puste dla x1;
- wallet identity jest puste;
- pre/post signer balances są brakujące;
- BUY wallet debit jest niedodatni;
- canonical order jest niekompletny;
- brakuje pełnego okna 8 s;
- są mniej niż cztery first buys;
- wynik jest non-finite.

Nie ma fallbacków ani imputacji.

---

## 8. Kalibracja bez modelu

Pierwsze:

```text
250 feature-evaluable births
```

służy wyłącznie do zamrożenia skali i progu. Są oznaczane:

```text
CALIBRATION_EXCLUDED
```

i nie wchodzą do finalnych średnich ani median.

Dla każdej cechy:

```text
z_i = clip((x_i - median_i) / IQR_i, -3, +3)
```

Jeżeli `IQR_i == 0`, capture jest `ACE_PROBE_INCONCLUSIVE`, a nie naprawiany arbitralnym epsilonem.

Score:

```text
score = -z1 + z2 + z3 + z4 - z5
```

Threshold:

```text
selected_threshold = 80. percentyl score pierwszych 250 rows
```

Po zamrożeniu:

```text
SELECTED = score >= selected_threshold
REST     = score < selected_threshold
```

`calibration_v1.json` zapisuje wyłącznie:

- five medians;
- five IQRs;
- score weights;
- threshold;
- cutoff;
- feature contract version;
- amount-source label;
- capacity limits;
- sustained-outcome parameters;
- source run ID i baseline SHA.

Dzień 2 używa tego samego pliku bez recalibration.

---

## 9. Typed quote proxy — istniejący Rust contract

Probe czyta `pump_quote_authority` z capture manifest i wywołuje:

```text
RugScalpPumpQuoteAuthorityV1::materialize()
RugScalpPumpQuoteContractV1::quote_buy_v2_under_wallet_cap(...)
RugScalpPumpQuoteContractV1::executable_exit_value_lamports(...)
```

Nie ma Pythonowej kopii matematyki.

### 9.1. Entry cap

```text
TOTAL_WALLET_DEBIT_CAP = 150_000_000 lamportów

entry_tx_cost =
    quote_contract.entry_transaction_cost_lamports()

program_cap =
    TOTAL_WALLET_DEBIT_CAP - entry_tx_cost
```

Typed entry:

```text
quote_buy_v2_under_wallet_cap(
    entry_state.slot,
    entry_state.reserves,
    program_cap
)
```

```text
entry_total_debit =
    buy_quote.program_settlement.wallet_debit_or_credit
    + entry_tx_cost
```

Twardy invariant:

```text
entry_total_debit <= 150_000_000
```

### 9.2. Capacity bounds

Te same liczby co w zatwierdzonym V2:

```text
entry self-impact                 <= 5%
immediate full-position exit impact <= 5%
entry_total_debit / x3_lamports  <= 10%
```

Runtime slippage ceiling 25% jest zakazany jako validity bound.

Przekroczenie któregokolwiek limitu:

```text
NON_EVALUABLE_CAPACITY
```

Notional nie jest zmniejszany.

### 9.3. Charakter proxy

Własny typed entry/exit quote i costs są liczone przez istniejący kontrakt.

Późniejsza observed market path nie jest przeliczana po hipotetycznym buyu. Dlatego wynik pozostaje:

```text
observed_path_non_propagated_sustained_proxy
```

---

## 10. Sustained economic outcome

Zamrożone parametry:

```text
PRIMARY_EXIT_LATENCY_MS = 250
SUSTAIN_CONFIRM_AT_MS   = 1_000
MAX_STATE_LOOKUP_LAG_MS = 1_000
OUTCOME_HORIZON_MS      = 120_000
```

Dla każdego full-reserve, pre-migration trigger state `T` po cutoffie:

1. landing state = pierwszy state o `event_ts_ms >= T + 250 ms`;
2. landing state musi wystąpić do `T + 1_250 ms`;
3. confirmation state = pierwszy state o `event_ts_ms >= T + 1_000 ms`;
4. confirmation state musi wystąpić do `T + 2_000 ms`;
5. landing i confirmation muszą mieć różne sloty;
6. oba muszą mieć `complete == false`;
7. oba muszą leżeć przed `cutoff + 120_000 ms`.

Dla obu stanów:

```text
exit_net_value =
    executable_exit_value_lamports(
        state.slot,
        state.reserves,
        entry_token_amount_raw
    )

net_return =
    (exit_net_value - entry_total_debit)
    / entry_total_debit
```

Sustained value triggera:

```text
sustained_return_T =
    min(net_return_landing, net_return_confirmation)
```

Jeden outcome birth:

```text
best_sustained_proxy_net_return_120s =
    max(sustained_return_T)
```

oraz:

```text
sustained_net17_hit =
    best_sustained_proxy_net_return_120s >= 0.17
```

Pojedynczy spike bez późniejszego potwierdzenia nie jest sukcesem.

Brak jakiejkolwiek legalnej pary landing/confirmation:

```text
NON_EVALUABLE_SUSTAIN_COVERAGE
```

Migracja (`complete == true`) nie jest wyceniana przez Pump curve route i nie tworzy fikcyjnego exitu. Jeżeli po cutoffie nie ma legalnego pre-migration sustained pair, row pozostaje non-evaluable sustain coverage.

---

## 11. Jeden terminalny row na birth

```json
{
  "schema": "ace_core_one_day_probe_v3",
  "candidate_id": "...",
  "base_mint": "...",
  "bonding_curve": "...",
  "creator": "...",
  "birth_ts_ms": 0,
  "cutoff_ts_ms": 0,

  "x1_creator_buy_wallet_debit_share": 0.0,
  "x2_new_buyer_intensity_log_ratio": 0.0,
  "x3_first_buy_wallet_debit_lamports": 0,
  "x4_first_buy_late_early_log_ratio": 0.0,
  "x5_first_buy_wallet_debit_hhi": 0.0,

  "score": 0.0,
  "selected": true,

  "entry_state_slot": 0,
  "entry_total_debit_lamports": 0,
  "entry_token_amount_raw": 0,
  "entry_impact_bps": 0,
  "immediate_exit_impact_bps": 0,

  "best_sustained_proxy_net_return_120s": 0.0,
  "best_trigger_ts_ms": 0,
  "landing_ts_ms": 0,
  "confirmation_ts_ms": 0,
  "sustained_net17_hit": false,

  "status": "EVALUABLE_SELECTED",
  "reason": null,
  "outcome_label": "observed_path_non_propagated_sustained_proxy"
}
```

---

## 12. Jedyne główne obliczenie

Dla rows po kalibracji:

```text
selected_mean =
    mean(outcome | EVALUABLE_SELECTED)

rest_mean =
    mean(outcome | EVALUABLE_REST)

DELTA_MEAN =
    selected_mean - rest_mean
```

Kontrola odporna na jeden outlier:

```text
selected_median =
    median(outcome | EVALUABLE_SELECTED)

rest_median =
    median(outcome | EVALUABLE_REST)

DELTA_MEDIAN =
    selected_median - rest_median
```

Dodatkowo tylko diagnostycznie:

```text
selected_sustained_net17_hit_rate
rest_sustained_net17_hit_rate
DELTA_SUSTAINED_HIT17
```

Bez p-value, bootstrapu, CI, Ridge i model selection.

---

## 13. Dzień 1

Capture trwa jeden ciągły okres 24 h.

Po capture uruchamiany jest offline probe.

### `ACE_PROBE_PROMISING_NOT_PROVEN`

Jeżeli:

```text
DELTA_MEAN > 0
DELTA_MEDIAN > 0
selected_mean > 0
selected_count >= 50
evaluable_coverage_pct >= 50%
```

STOP. Strategia przeżyła tani filtr.

Nie oznacza to executable EV ani zgody na kapitał.

### `ACE_PROBE_DAY1_NEGATIVE_UNCONFIRMED`

Jeżeli:

```text
DELTA_MEAN <= 0
AND
DELTA_MEDIAN <= 0
```

Dopuszczalny jest dokładnie jeden Dzień 2 z niezmienioną kalibracją.

### `ACE_PROBE_DAY1_MIXED`

Jeżeli znaki są przeciwne albo count/coverage floor nie został osiągnięty.

Dopuszczalny jest dokładnie jeden Dzień 2.

---

## 14. Dzień 2 — tylko gdy potrzebny

Drugi capture:

- ma nowy run ID i output paths;
- trwa kolejne niezależne 24 h;
- używa dokładnie `calibration_v1.json` z Dnia 1;
- nie przelicza median, IQR ani threshold;
- nie zmienia cech, score, capacity ani outcome.

### Finalne `ACE_PROBE_DEAD`

Tylko gdy oba dni niezależnie mają:

```text
DELTA_MEAN <= 0
AND
DELTA_MEDIAN <= 0
```

### Finalne `ACE_PROBE_PROMISING_NOT_PROVEN`

Jeżeli pooled test rows spełniają:

```text
pooled DELTA_MEAN > 0
pooled DELTA_MEDIAN > 0
pooled selected_mean > 0
pooled selected_count >= 100
pooled evaluable_coverage_pct >= 50%
```

### `ACE_PROBE_INCONCLUSIVE`

Każdy inny wynik.

Nie ma trzeciego dnia w tym planie.

---

## 15. Raport

Dla każdego dnia oraz pooled, jeżeli był Dzień 2:

```text
baseline_sha
run_id
authority_epoch_id
fee_authority_evidence_hash
birth_count
calibration_excluded_count
selected_count
rest_count
non_evaluable_count_by_reason
evaluable_coverage_pct

selected_mean
rest_mean
DELTA_MEAN

selected_median
rest_median
DELTA_MEDIAN

selected_sustained_net17_hit_rate
rest_sustained_net17_hit_rate
DELTA_SUSTAINED_HIT17
```

Żadnych dodatkowych modeli ani analiz przed odczytaniem tego wyniku.

---

## 16. Focused tests przed capture

Wymagane są wyłącznie testy chroniące wynik:

1. `PoolTransactionPayload` zachowuje signer pre/post balances.
2. Missing balance nie używa `sol_amount_lamports` jako fallbacku.
3. BUY wallet debit jest `pre - post`; zero/ujemny debit jest non-evaluable.
4. `effective_curve_quote_lamports` nie jest odczytywany przez ACE probe.
5. Trade po cutoffie nie zmienia żadnej z x1–x5.
6. Outcome state niepóźniejszy niż cutoff jest ignorowany.
7. Multi-mutation same-signature nie jest scalana po samej signature.
8. Pierwsze 250 feature-evaluable births są wykluczone z finalnych delt.
9. Dzień 2 odrzuca brak albo mismatch calibration file.
10. `entry_total_debit <= 150_000_000` zawsze.
11. Probe materializuje quote contract z capture manifest, nie z późniejszego RPC.
12. Existing BuyV2/LegacySell fixture daje exact parity w probe.
13. Entry impact i immediate exit impact są osobne.
14. 5%/5%/10% violation daje `NON_EVALUABLE_CAPACITY`; notional nie jest zmniejszany.
15. Single-slot spike bez confirmation slot nie daje sustained hit.
16. Landing `>=17%`, confirmation `<17%` daje `sustained_net17_hit=false`.
17. `enable_optional_events=false` jest odrzucane przez capture preflight.
18. Ten sam tape + manifest + calibration daje bit-identyczne rows i summary.

Nie uruchamiamy szerokiej kampanii CI ani review produkcyjnego jako warunku tego eksperymentu. Wystarczą focused tests, `cargo fmt`, build probe binary i jeden krótki fixture smoke.

---

## 17. Komendy operacyjne

### 17.1. Build

```bash
cargo fmt --all --check
cargo test -p ghost-launcher ace_core_one_day_probe --lib -- --nocapture
cargo build --release -p ghost-launcher --bin ghost-launcher --bin ace_core_one_day_probe
```

### 17.2. Capture Dnia 1

```bash
./target/release/ghost-launcher \
  --config configs/rollout/ace-core-one-day-probe-r1.toml
```

Po kontrolowanym zakończeniu capture:

```bash
./target/release/ace_core_one_day_probe \
  --events-dir <DAY1_EVENTS_DIR> \
  --manifest <DAY1_MANIFEST> \
  --output-dir <DAY1_PROBE_DIR> \
  --day-id day1
```

### 17.3. Dzień 2, tylko jeżeli wymagany

```bash
./target/release/ace_core_one_day_probe \
  --events-dir <DAY2_EVENTS_DIR> \
  --manifest <DAY2_MANIFEST> \
  --output-dir <DAY2_PROBE_DIR> \
  --day-id day2 \
  --calibration <DAY1_PROBE_DIR>/calibration_v1.json
```

---

## 18. STOP

Plan kończy się jednym z:

```text
ACE_PROBE_PROMISING_NOT_PROVEN
ACE_PROBE_DEAD
ACE_PROBE_INCONCLUSIVE
```

Dopiero `ACE_PROBE_PROMISING_NOT_PROVEN` pozwala podjąć osobną decyzję, czy warto wrócić do cięższej walidacji.

W ramach tego planu nie uruchamia się automatycznie:

- PR2 ingest–state–quote;
- nowych cech;
- innych wag;
- innego cutoffu;
- innego notionalu;
- innej polityki wyjścia;
- exact landing/failure replay;
- Position Managera;
- bootstrapu;
- modelu ML;
- live ACE.

Negatywny lub nierozstrzygający wynik jest prawidłowym końcem eksperymentu.
