# PLAN: Shadow V2 Fidelity Validation Burnin PR12

Data: 2026-06-30

Status:

```text
PR12_PLAN_ONLY_READY_FOR_REVIEW
```

## 1. Cel

PR12 definiuje plan przyszłego burninu walidacyjnego Shadow Burnin Simulation V2.
Ten dokument nie jest wynikiem runu i nie jest dowodem strategii. Jego jedynym
celem jest zamknięcie kontraktu: jakie artefakty, metryki, manifesty i bramki
muszą istnieć, zanim Shadow V2 będzie można nazwać research-grade.

PR12 ma przygotować:

- fidelity validation scope;
- wymagane pre-run i post-run manifesty;
- wymagane artefakty canonical/replay/lifecycle/density;
- bramki rekonstrukcji entry/exit;
- bramki reconciliation replay/lifecycle;
- bramki path density dla 2s/3s, 120s, 300s i 500s;
- golden traces inspectability;
- jawny downgrade boundary dla starych raportów.

## 2. Non-Goals

PR12 nie jest:

- strategy proof;
- RCE proof;
- selector proof;
- edge proof;
- runtime approval proof;
- live-equivalence proof;
- zgodą na `shadow_close_only`;
- zgodą na active close;
- zgodą na live TX/Jito path;
- zgodą na uruchomienie R51 albo interpretację R51 jako strategy evidence.

PR12 nie uruchamia żadnego runu. Start burninu wymaga osobnej dyspozycji
operatora po review planu.

## 3. Wejściowy Stan Prawdy

Root verdict z P0 audit:

```text
SHADOW_REPLAY_LIFECYCLE_MISMATCH
```

Konsekwencje:

- Shadow V1 nie jest live-equivalent;
- lifecycle V1 i `shadow_exit_replay_v1` nie są jedną prawdą pozycji;
- entry price V1 nie jest udowodniony jako live fill;
- exit price V1 jest offline mark/path evidence, nie sell fill;
- 2s/3s V1 to co najwyżej sparse approximation;
- 300s/500s V1 nie są ewaluowalne bez pokrycia horizon;
- stare raporty nie mogą być cytowane jako proof live PnL.

## 4. Kontrakt Runu Walidacyjnego

Planowany przyszły run musi być opisany jako:

```text
validation_mode=FIDELITY_ONLY
plan_status=PLAN_ONLY until explicit run approval
logging_only=true
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
strategy_proof_enabled=false
rce_proof_enabled=false
selector_proof_enabled=false
edge_proof_enabled=false
```

Kontrakt statyczny znajduje się w:

```text
configs/rollout/shadow_v2_fidelity_validation_burnin_plan.toml
```

Ten plik nie jest aktywnym runtime configiem. Służy do statycznej walidacji
scope PR12.

## 5. Wymagane Tryby Ścieżki

Run walidacyjny musi zebrać lub jawnie zablokować:

- `shadow_path_dense_3s`;
- `shadow_path_standard_120s`;
- `shadow_path_long_500s`.

Wymagane horizon checks:

- 2000 ms;
- 3000 ms;
- 10000 ms;
- 30000 ms;
- 120000 ms;
- 300000 ms;
- 500000 ms.

Nie wolno inferować 300s/500s, jeżeli long horizon nie ma realnego coverage.
Wynik musi być `NOT_EVALUABLE_*`, nie domysł.

## 6. Wymagane Artefakty

Przyszły run walidacyjny musi wygenerować co najmniej:

- `pre_run_manifest.json`;
- `post_run_manifest.json`;
- `shadow_position_event_v2.jsonl`;
- `shadow_replay_v2.jsonl`;
- `shadow_lifecycle_v2.jsonl`;
- `shadow_path_density_v2.jsonl`;
- `shadow_v2_manifest_report.csv`;
- `shadow_v2_fidelity_validation_report.md`;
- `shadow_v2_golden_traces_manifest.csv`.

Raw JSONL/logs nie są commitowane. Commitowane mogą być tylko pochodne raporty,
manifesty i dokumenty dopuszczone przez review.

## 7. Pre-Run Manifest Gate

Przed przyszłym runem musi istnieć pre-run manifest z:

- `run_id`;
- `simulation_contract_version`;
- config fingerprint albo jawny config path;
- git commit;
- expected artifact list;
- expected schema list;
- retention policy;
- storage budget;
- path modes;
- max horizon;
- forbidden proof flags.

Brak pre-run manifestu oznacza:

```text
SHADOW_V2_VALIDATION_PRE_RUN_MANIFEST_MISSING
```

## 8. Post-Run Manifest Gate

Po przyszłym runie musi istnieć post-run manifest z:

- sha256 dla artefaktów albo jawny `SKIPPED_TOO_LARGE`;
- row counts;
- malformed JSONL counts;
- schema coverage;
- symlink status;
- missing required artifact list;
- raw JSONL staging forbidden flag;
- exact command/version used for audit.

Brak post-run manifestu oznacza:

```text
SHADOW_V2_MANIFEST_INCOMPLETE
```

## 9. Fidelity Gates

Minimalne bramki research-grade:

- entry reconstruction coverage >= 99%;
- exit reconstruction coverage >= 99%;
- lifecycle/replay terminal reconciliation >= 99%;
- duplicate terminal records = 0 albo typed sub-events;
- ambiguous fallback joins accepted silently = 0;
- critical temporal leakage findings = 0;
- research timestamp fields without clock domain = 0;
- required records without event_order_key = 0;
- path density report exists for every required horizon;
- unsupported horizons are marked `NOT_EVALUABLE`;
- simulator fixtures pass;
- golden traces are manually inspectable;
- pre/post manifests complete.

Jeżeli którakolwiek bramka nie przejdzie, maksymalny status to:

```text
SHADOW_V2_VALIDATION_BLOCKED
```

## 10. Live-Equivalence Boundary

Bez PR14 live-confirmed calibration dataset maksymalny verdict pozostaje:

```text
SHADOW_V2_RESEARCH_GRADE_ONLY
```

Nawet jeżeli PR12 future validation burnin przejdzie wszystkie bramki
research-grade, nie wolno twierdzić:

- live-equivalent PnL;
- executable live fill;
- real landing outcome;
- live slippage behavior;
- calibrated quote/fill divergence.

## 11. Static Guard

Plan PR12 jest sprawdzany przez:

```text
python3 scripts/shadow_v2_validation_burnin_plan_audit.py
PYTHONDONTWRITEBYTECODE=1 python3 scripts/test_shadow_v2_validation_burnin_plan_audit.py
```

Guard sprawdza, że:

- plan jest `PLAN_ONLY`;
- `validation_mode=FIDELITY_ONLY`;
- run start jest zabroniony;
- wszystkie approval/proof flags są false;
- PR14 jest wymagany dla live-equivalence;
- wymagane horizon/gates/artifacts są zadeklarowane.

## 12. Decyzja

PR12 może być zaakceptowany wyłącznie jako plan przyszłej walidacji fidelity.
Nie odblokowuje strategii i nie zmienia statusu starych raportów.

Final PR12 status po merge:

```text
PR12_PLAN_READY_NO_RUN_STARTED
```
