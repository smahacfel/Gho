# ADR-8D: Shadow Burnin V2 PR14 Live-Confirmed Calibration Contract

Data: 2026-06-30

Status:

```text
PR14_READY_FOR_REVIEW_CONTRACT_READY_REAL_DATASET_PENDING
```

## D1. Problem

Po PR1-PR13 Shadow V2 ma kontrakty schema, canonical truth, pool-state
provenance, entry/exit fill static models, path sampler, replay/lifecycle V2,
manifesty, logging-only validation config, plan fidelity validation burnin i
downgrade enforcement dla V1.

Nadal brakowalo ostatniej warstwy wymaganej do jakiegokolwiek
`SHADOW_V2_LIVE_EQUIVALENCE_GRADE`: live-confirmed calibration dataset.

Bez tego datasetu Shadow V2 moze co najwyzej byc research-grade po przejsciu
osobnych fidelity gates. Nie wolno twierdzic, ze static fill model jest live
fill, ze quote jest fill, albo ze simulated landing odpowiada realnemu
potwierdzonemu landingowi.

## D2. Decision

Dodajemy PR14 jako statyczny kontrakt i offline audit:

- `configs/rollout/shadow_v2_live_confirmed_calibration_contract.toml`;
- `reports/selector/shadow_v2_live_calibration_schema_manifest.csv`;
- `reports/selector/shadow_v2_live_calibration_gap_matrix.csv`;
- `scripts/shadow_v2_live_calibration_audit.py`;
- `scripts/test_shadow_v2_live_calibration_audit.py`;
- raport PR14 w `PLANS/AUDYT`;
- aktualizacje spec, gates, workbreakdown, schema manifest i risk register.

PR14 nie tworzy sztucznego live datasetu. Jezeli realny dataset nie istnieje,
audit zwraca `CONTRACT_READY`, ale `pr14_calibration_gate_pass=false` i
`live_equivalence_grade_allowed=false`.

## D3. Evidence

Evidence dodane w PR14:

- kontrakt TOML z `enabled=false`, `run_start_allowed=false`,
  `live_collection_enabled=false`, `tx_path_changes_allowed=false`;
- schema manifest dla:
  - `live_calibration_manifest_v1`;
  - `live_transaction_attempt_v1`;
  - `live_confirmed_entry_fill_v1`;
  - `live_confirmed_exit_fill_v1`;
  - `live_calibration_comparison_v1`;
- gap matrix pokrywajaca latency, landing slot, failure/no-fill, slippage,
  own impact, fees, quote/fill divergence, state/stream delay i model error;
- offline audit PR14;
- fixture tests dla pass i block cases.

## D4. Root Cause

Shadow V1 mieszal mark/path evidence z lifecycle evidence i nie byl
live-equivalent. PR4-PR7 w Shadow V2 dodaly deterministic/static fill
simulation, ale static model nadal nie jest live-confirmed truth.

Brakowalo twardego mechanizmu, ktory wymusza:

- realne `decision_ts_ms -> submit_ts_ms -> landing_ts_ms`;
- realny `landing_slot`;
- `FILLED`/`NO_FILL`/`FAILED` status;
- realized slippage zamiast configured tolerance;
- quote/fill divergence;
- own impact;
- fees/tips;
- account-state i stream delay;
- calibrated model error.

## D5. Corrective Action

PR14 wprowadza:

- `shadow_v2_live_confirmed_calibration_contract_v1`;
- wymagane pliki datasetu;
- wymagane record schemas;
- wymagane telemetry fields;
- offline audit datasetu;
- fixture tests;
- acceptance gates:
  - `GATE_PR14_CALIBRATION_CONTRACT`;
  - `GATE_PR14_FIXTURES`;
  - `GATE_PR14_CALIBRATION`;
  - `GATE_QUOTE_FILL_DIVERGENCE`;
- risk register rows dla braku datasetu, misclassification i brakujacej
  latency/failure/slippage calibration.

## D6. Rejected Alternatives

Odrzucono:

- uruchomienie live runu w PR14;
- dotykanie TX/Jito/live path;
- wlaczenie live collection przez config;
- traktowanie fixture datasetu jako live-confirmed evidence;
- commitowanie raw JSONL kalibracji;
- podniesienie Shadow V2 do live-equivalence-grade bez realnego datasetu;
- uruchamianie RCE proof, selector proof albo strategy proof.

## D7. Consequences

Po PR14:

- repo ma gotowy kontrakt i walidator datasetu live-confirmed calibration;
- realny dataset moze byc dostarczony pozniej i sprawdzony offline;
- brak datasetu nie blokuje contract readiness, ale blokuje live-equivalence;
- `SHADOW_V2_LIVE_EQUIVALENCE_GRADE` pozostaje nieprzyznany;
- maksymalny verdict bez datasetu pozostaje:

```text
SHADOW_V2_RESEARCH_GRADE_ONLY
```

Runtime boundary pozostaje:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
```

## D8. Verification

Wymagane lokalne sprawdzenia PR14:

```text
python3 -m py_compile scripts/shadow_v2_live_calibration_audit.py scripts/test_shadow_v2_live_calibration_audit.py
python3 scripts/shadow_v2_live_calibration_audit.py --help
python3 scripts/shadow_v2_live_calibration_audit.py --strict
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_shadow_v2_live_calibration_audit.py
python3 scripts/shadow_v2_live_calibration_audit.py --require-dataset
git diff --check
git diff --cached --check
forbidden staged-file guard
```

Oczekiwane:

- default/strict contract audit: `CONTRACT_READY`;
- fixture tests: PASS;
- `--require-dataset` bez datasetu: BLOCKED;
- brak runtime changes;
- brak run start;
- brak R51 touch;
- brak raw JSONL/log staging.
