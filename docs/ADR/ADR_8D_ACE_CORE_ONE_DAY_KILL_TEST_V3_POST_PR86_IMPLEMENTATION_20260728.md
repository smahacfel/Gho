# ADR-8D: ACE Core — One-Day Kill Test V3 po PR #86

Status: `IMPLEMENTED_LOCALLY / FOCUSED_VALIDATION_PASS / CAPTURE_NOT_STARTED /
OBSERVE_ONLY / PR2_STILL_BLOCKED`

Typ: ADR-8D / offline falsification probe / durable observe-only evidence

Data: `2026-07-28`

Repo: `smahacfel/Gho`

Baseline: `origin/main = 43057b296663129ca9b4f572e793474830a5452c`

Plan SSOT:
`PLANS/DO_REALIZACJI/PLAN_ACE_CORE_ONE_DAY_KILL_TEST_V3_POST_PR86.md`

Uwaga o szablonie: literalna ścieżka wskazana w instrukcji globalnej,
`/Gho/docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten
dokument zachowuje lokalny format ADR-8D stosowany w `docs/ADR/`.

## D0. Decyzja

Zaimplementowano wyłącznie najmniejszy, offline-only probe falsyfikacyjny
ACE Core V3. Jego jedyna analiza to porównanie `SELECTED` z `REST` po
zamrożeniu kalibracji z pierwszych 250 feature-evaluable births.

Probe nie tworzy runtime reducera, observera, event-bus subscribera, entry
intent, lifecycle, modelu ML, replayu portfelowego ani ścieżki live/shadow
execution. Nie zmienia `MaterializedFeatureSet`, Gatekeepera, quote math,
route authorization, Position Managera ani PR2 ingest–state–quote.

Nie uruchomiono capture 24 h. Ten ADR nie jest wynikiem badania sygnału i nie
autoryzuje kapitału ani PR2.

## D1. Durable evidence i frozen authority

`PoolTransactionPayload` zachowuje teraz addytywnie:

```text
signer_pre_balance_lamports
signer_post_balance_lamports
is_synthetic
```

Pola pre/post są przekazywane bez transformacji z istniejącego
`PoolTransaction`. `is_synthetic` jest dodatkową provenance potrzebną do
wykonania jawnego warunku planu, że feature i reserve rows muszą być
niesyntetyczne. Brak tego pola nie staje się `false`; jest non-evaluable dla
ACE.

Offline probe wyznacza jedyny amount-based flow jako:

```text
observed_buy_wallet_debit_lamports =
    signer_pre_balance_lamports - signer_post_balance_lamports
```

wyłącznie dla successful, canonical-order-complete, observed BUY z dodatnim
debit. Nie odczytuje `sol_amount_lamports`,
`effective_curve_quote_lamports`, `volume_sol`, ceny ani token amount jako
fallbacku feature flow.

`RugRealityCaptureRunManifestV1` ma schema v2 i utrwala:

```text
authority_epoch_id
event_writer_run_id
event_writer_optional_events_enabled
pump_quote_authority: RugScalpPumpQuoteAuthorityV1
```

Ten typed authority powstaje przy starcie capture z istniejącej materializacji
on-chain authority. Probe odtwarza kontrakt przez `materialize()` z manifestu;
nie pobiera później fee schedule przez RPC ani nie implementuje matematyki w
innym języku.

## D2. Capture contract

Nowa konfiguracja
`configs/rollout/ace-core-one-day-probe-r1.toml` ma odrębne run ID, manifest,
event directory oraz log paths. Utrzymuje:

```text
trigger.enabled = false
execution_mode = shadow
p37_shadow_probe.enabled = false
rug_scalp_v2.enabled = false
rug_reality_capture.enabled = true
execution.events.enable_optional_events = true
execution.events.enable_aem_ticks = false
```

Capture preflight odrzuca `rug_reality_capture.enabled=true` przy
`enable_optional_events=false`, ponieważ `PoolTransaction` jest eventem
opcjonalnym EventWritera.

Istniejący launcher wymaga `trigger.shadow_run.enabled=true`, gdy
`execution_mode=shadow` i `entry_mode=shadow_only`. Konfiguracja zachowuje tę
wyłącznie strukturalną zależność transportu, ale `trigger.enabled=false`
pozostaje twardym wyłączeniem dispatchu. Nie jest to uruchomienie shadow
entry.

Dla capture-only Oracle Runtime otrzymuje z manifestu dokładny run ID i
`EventWriterConfig`; zwykła ścieżka runtime zachowuje historyczne domyślne
zachowanie. Dzięki temu durable tape i manifest nie mogą przypadkowo opisywać
różnych runów, a ustawienie optional events jest faktycznie stosowane, a nie
jedynie zapisane w TOML.

## D3. Offline contract probe

Nowy moduł `ace_core_one_day_probe` i thin binary przyjmują tylko:

```text
--events-dir
--manifest
--output-dir
--day-id day1|day2
--calibration (tylko day2)
```

Czytane są durable `exec_*.jsonl`, z wymaganym newline flush, poprawnym JSON,
jednym manifest run ID oraz `Lane::Shadow`. Probe wybiera najwcześniejszy
canonical birth dla `(base_mint, bonding_curve)`, liczy duplicate evidence,
a trades deduplikuje wyłącznie po pełnym kluczu:

```text
signature + slot + tx_index + outer_instruction_index +
inner_group_index + event_ordinal
```

Brak pełnego order key nie jest scalany. Jeżeli taki successful BUY leży w
feature window, daje `NON_EVALUABLE_FEATURES`.

Feature cutoff wynosi dokładnie `birth_ts_ms + 11_111`. Cechy x1–x5 używają
wyłącznie BUY wallet-debit i nie mogą odczytać state'u po cutoffie. Entry
state musi być kompletnym, `complete=false`, niesyntetycznym reserve state
nie późniejszym niż cutoff i świeższym niż 1 s. Stan po cutoffie służy
wyłącznie sustained outcome.

Typed entry ma total wallet-debit cap `150_000_000` lamportów. Probe stosuje
oddzielne 5% bounds dla entry i pełnego immediate exit oraz 10% bound
`entry_total_debit / x3`; nie zmniejsza notionals. Sustained proxy wymaga
legalnej pary landing/confirmation w różnych slotach i pozostaje nazwany:

```text
observed_path_non_propagated_sustained_proxy
```

## D4. Kalibracja, statusy i wyniki

Dzień 1 wyłącza pierwsze 250 feature-evaluable births z porównania i zapisuje
`calibration_v1.json`. Zerowy lub non-finite IQR kończy się
`ACE_PROBE_INCONCLUSIVE`; nie ma epsilonu. Dzień 2 wymaga tego samego
calibration file, odrzuca brak, mismatch kontraktu, inny baseline oraz ten sam
run ID. Przed poolingiem dodatkowo wymaga odpowiadającego mu kompletnego
summary Dnia 1 o statusie negative albo mixed.

Rows mają wyłącznie planową taksonomię:

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

`CALIBRATION_EXCLUDED` jest raportowane osobno i nie zanieczyszcza
`non_evaluable_count_by_reason`. Summary zwraca wymagane count/coverage,
mean, median oraz sustained-net17 hit-rate; pooled metrics powstają wyłącznie
dla legalnego Dnia 2.

## D5. Capture validity i granica operacyjna

Probe mechanicznie odrzuca niezgodny manifest/tape, schema bez frozen quote
authority, brak optional transaction events, błędny lane/run ID, uszkodzony
JSONL oraz niedomkniętą linię writer'a.

Plan wymaga też istniejących PR1E health facts, których nie ma w argumentach
CLI ani w manifeście: global candidate-admission closure,
`pr1_runtime_bypass_attempt_total > 0`, primary local coverage gap oraz
kontrolowane zakończenie pełnego 24 h capture. Nie utworzono dla nich nowego
health frameworka ani sztucznego fallbacku. Operator musi przed probe
potwierdzić je z istniejących PR1E logs/counters oraz kontrolowanego flushu;
w przeciwnym razie cały dzień jest `INVALID_CAPTURE` zgodnie z planem.

## D6. Weryfikacja implementacji

Focused test suite chroni w szczególności:

1. propagację pre/post balances i brak fallbacku amount;
2. dodatni wallet-debit oraz ignorowanie skażonego curve quote;
3. cutoff safety i full mutation dedupe;
4. pierwsze 250 calibration exclusions i zamrożony Dzień 2;
5. frozen typed authority oraz parity BuyV2;
6. osobne entry/exit impacts, cap i three capacity bounds;
7. sustained confirmation/different-slot semantics;
8. optional-event preflight, rollout decode i bit-identyczne outputy.

Przed przekazaniem implementacji wykonywane są wyłącznie planowe narrow
checks:

```bash
cargo fmt --all --check
cargo test -p ghost-launcher ace_core_one_day_probe --lib -- --nocapture
cargo check -p ghost-launcher
cargo build --release -p ghost-launcher --bin ghost-launcher --bin ace_core_one_day_probe
git diff --check
```

Ostrzeżenia istniejące w dużych crate'ach nie są traktowane jako wynik ACE;
komendy muszą zakończyć się kodem 0.

Wykonano lokalnie na branchu
`agent/ace-core-one-day-kill-test-v3`:

```text
cargo fmt --all --check                                      PASS
cargo test -p ghost-launcher ace_core_one_day_probe --lib    PASS (22/22)
cargo check -p ghost-launcher                                PASS
cargo build --release -p ghost-launcher --bin ghost-launcher
  --bin ace_core_one_day_probe                               PASS
ace_core_one_day_probe --help                                PASS
git diff --check oraz kontrola nowych plików                 PASS
```

## D7. Nieautoryzowane następne kroki i rollback

Ten change set nie autoryzuje PR2, dodatkowych features, zmian wag, innego
cutoffu/notionalu, exact executable replay, bootstrapu, ML, Position Managera
ani live ACE. Po capture jedynym dozwolonym wynikiem operacyjnym jest jeden z:

```text
ACE_PROBE_PROMISING_NOT_PROVEN
ACE_PROBE_DEAD
ACE_PROBE_INCONCLUSIVE
```

Rollback jest prosty: wyłączyć/usunąć nowy rollout i nie uruchamiać binarnego
probe'u. Żaden aktywny Gatekeeper, MFS, decision path ani execution policy nie
został przez tę implementację przełączony.
