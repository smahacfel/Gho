# RAPORT: Shadow V2 Live-Confirmed Calibration PR14

Data: 2026-06-30

Status:

```text
PR14_READY_FOR_REVIEW_CONTRACT_READY_REAL_DATASET_PENDING
```

## 1. Cel

PR14 zamyka ostatni brakujacy kontrakt Shadow Burnin Simulation V2: warstwe
live-confirmed calibration dataset. Ta warstwa jest wymagana zanim jakikolwiek
Shadow V2 model fill/latency/slippage/impact bedzie mogl byc nazwany
live-equivalence-grade.

PR14 nie uruchamia runu, nie zbiera live danych, nie dotyka R51, nie zmienia
BUY/REJECT, Gatekeeper policy, selector runtime ani TX/Jito/live path.

## 2. Decyzja

Dodajemy statyczny kontrakt PR14:

```text
configs/rollout/shadow_v2_live_confirmed_calibration_contract.toml
reports/selector/shadow_v2_live_calibration_schema_manifest.csv
reports/selector/shadow_v2_live_calibration_gap_matrix.csv
scripts/shadow_v2_live_calibration_audit.py
scripts/test_shadow_v2_live_calibration_audit.py
```

Kontrakt definiuje wymagane artefakty datasetu:

- `live_calibration_manifest.json`;
- `live_transaction_attempts.jsonl`;
- `live_confirmed_entry_fills.jsonl`;
- `live_confirmed_exit_fills.jsonl`;
- `live_calibration_comparison.jsonl`.

Te pliki nie sa commitowane w PR14 jako raw evidence. Sa tylko schematem
przyszlego lokalnego datasetu kalibracyjnego.

## 3. Co PR14 Udowadnia

PR14 udowadnia tylko to, ze repo ma:

- jawny kontrakt PR14;
- wymagane pola live-confirmed telemetry;
- jawne clock-domain dla timestampow;
- offline audit datasetu;
- fixture tests dla pass/block przypadkow;
- bramki acceptance, ktore oddzielaja contract readiness od realnego datasetu.

PR14 nie udowadnia, ze live-confirmed dataset juz istnieje.

## 4. Co Nadal Jest Zablokowane

Do czasu dostarczenia realnego datasetu i uruchomienia audytu:

```text
python3 scripts/shadow_v2_live_calibration_audit.py --dataset-root <live_confirmed_dataset> --require-dataset
```

zablokowane pozostaja:

- `SHADOW_V2_LIVE_EQUIVALENCE_GRADE`;
- live PnL proof;
- executable fill proof;
- real landing outcome proof;
- runtime approval;
- `shadow_close_only` approval;
- active close approval.

Maksymalny verdict bez datasetu:

```text
SHADOW_V2_RESEARCH_GRADE_ONLY
```

## 5. Wymagane Pola Kalibracji

Kontrakt wymaga co najmniej:

- `decision_ts_ms`;
- `submit_ts_ms`;
- `landing_ts_ms`;
- `decision_to_submit_ms`;
- `submit_to_land_ms`;
- `landing_slot`;
- `fill_status`;
- `failure_mode`;
- `quote_price`;
- `fill_price`;
- `realized_slippage_bps`;
- `quote_fill_diff_bps`;
- `own_impact_bps`;
- `fee_bps`;
- `priority_fee_lamports`;
- `jito_tip_lamports`;
- `account_state_delay_ms`;
- `stream_delay_ms`;
- calibrated `model_error_bps`.

Wymagane jest rozdzielenie:

- transaction attempt;
- live-confirmed entry fill;
- live-confirmed exit fill;
- comparison row pomiedzy modelem i live fill.

## 6. Audit Semantics

Domyslne uruchomienie:

```text
python3 scripts/shadow_v2_live_calibration_audit.py
```

waliduje tylko kontrakt i zwraca status:

```text
CONTRACT_READY
```

To oznacza: repo jest gotowe przyjac i sprawdzic dataset. Nie oznacza to
live-equivalence.

Tryb wymagajacy datasetu:

```text
python3 scripts/shadow_v2_live_calibration_audit.py --dataset-root <live_confirmed_dataset> --require-dataset
```

blokuje wynik, jezeli brakuje datasetu, wymaganych plikow, pol, latency
consistency, failure/no-fill evidence, entry/exit fill evidence albo comparison
rows.

## 7. Fixture Proof

Fixture tests sprawdzaja:

- brak datasetu daje `CONTRACT_READY`, ale nie live-equivalence;
- poprawny tymczasowy fixture dataset daje pass dla PR14 gate;
- brak wymaganego pliku blokuje;
- malformed JSONL blokuje;
- niespojne latency blokuje;
- `FAILED`/`NO_FILL` bez explicit `failure_mode` blokuje;
- `--require-dataset` bez datasetu blokuje.

## 8. Granice Runtime

PR14 zachowuje:

```text
NO_RUNTIME_SEMANTICS_CHANGED
NO_RUN_STARTED
NO_R51_TOUCH
NO_BUY_REJECT_CHANGE
NO_GATEKEEPER_POLICY_CHANGE
NO_SELECTOR_RUNTIME_CHANGE
NO_TX_JITO_LIVE_PATH_CHANGE
NO_SHADOW_CLOSE_ONLY_ENABLEMENT
NO_ACTIVE_CLOSE_ENABLEMENT
```

## 9. Decyzja Koncowa

PR14 jest gotowy do review jako contract/tooling PR.

Final PR14 verdict:

```text
PR14_READY_FOR_REVIEW_CONTRACT_READY_REAL_DATASET_PENDING
```

Shadow V2 nadal nie jest live-equivalence-grade. Realny live-confirmed
calibration dataset pozostaje wymaganym dowodem przed jakimkolwiek
`SHADOW_V2_LIVE_EQUIVALENCE_GRADE`.
