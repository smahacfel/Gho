# ADR-8D: HET-PM V2 PR B — remediation kontraktu promotion evidence

Data: 2026-07-17

Typ: ADR-8D / promotion evidence prerequisite / observe-only runtime telemetry / offline validation

Status: Accepted for PR #73 remediation

Uwaga proceduralna: literalny szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Dokument zachowuje lokalny format ADR-8D stosowany przez wcześniejsze ADR-y w `docs/ADR`.

## D1. Problem

Focused re-review PR #73 wskazał, że prerequisite promotion evidence nadal pozwalał na niepoprawne wnioski mimo zachowania V1-only authority:

- globalne metryki ekonomiczne mieszały Trailing i Vitality, więc jeden gate mógł ratować drugi;
- jedna pozycja mogła wystąpić jako dwa matched outcomes, gdy miała więcej niż jeden promoted candidate;
- manifest runu można było relabelować bez rekonstrukcji ze źródeł;
- validation protocol nie zamrażał jednoznacznie runtime/provenance contractu;
- admission evidence wykonywało synchroniczne filesystem I/O na aktywnej ścieżce PostBuyRuntime;
- admission reconciliation było głównie jednokierunkowe;
- unsupported route mógł być liczony jako dostępna executable route;
- brakujące dane ekonomiczne były pomijane zamiast fail-closed;
- dwa validation runy były liczone globalnie bez minimalnej per-run jakości;
- brakowało SLA `monitoring_registered -> first HET comparison`.

## D2. Zakres decyzji

Decyzja dotyczy wyłącznie promotion-evidence prerequisite PR #73.

Poza zakresem pozostają:

- authority cutover HET-PM V2;
- zmiana hierarchii live/shadow authority;
- zmiana V1 proposal/apply/terminal/capacity ownership;
- deploy/migration runtime.

## D3. Root cause

Pierwotny prerequisite miał poprawny kierunek architektoniczny, ale zbyt słaby contract denominator:

1. ekonomia była liczona na zbiorze occurrence, nie na combined-policy outcome per pozycja;
2. gate-specific sample counts nie wymuszały gate-specific economic pass;
3. manifest i launcher proof nie były traktowane jako rekalkulowany source contract;
4. admission stream był dowodem audytowym, ale zachowywał się jak synchroniczny runtime side-effect;
5. analyzer liczył tylko widoczne rekordy, bez wystarczającego fail-closed joinu dla missing/late evidence.

## D4. Decyzja

Wprowadzono następujące kontrakty:

1. Full gate-specific promotion economics dla każdego `promotion_requested=true` gate:
   - candidate count;
   - matched count;
   - mean/giveback/tail/CVaR/cost/trimmed metrics;
   - false-early proxy;
   - continuation coverage;
   - route availability;
   - censor count;
   - economic join failures.

2. Combined-policy view:
   - dokładnie jeden counterfactual outcome per `(run_id, position_id, position_epoch)`;
   - wybór pierwszego czasowo authority-eligible candidate;
   - przy tym samym ticku wybór według wersjonowanej hierarchii;
   - Crash pozostaje niepromowany i nie uczestniczy w combined-policy selection.

3. Fail-closed economic joins:
   - promoted candidate z terminal/replay, ale bez wymaganego pola, zwiększa typed failure;
   - promoted candidate bez expected terminal/replay i bez censoringu nie znika z próby.

4. Exact route allowlista:
   - executable route availability oznacza `pump_curve_supported`;
   - `curve_complete_pump_swap_unsupported` nie spełnia route availability.

5. Run-manifest source reconstruction:
   - `validate` rekalkuluje artifact z run manifestów;
   - run manifest jest rekalkulowany ze źródeł, launcher reportu i artifact hashes.

6. Frozen validation contract:
   - criteria v3 ma pola na expected runtime commit, binary hash, brain/run config hash, normalized entry cohort hash oraz dependency hashes;
   - prospective validation runy muszą użyć zamrożonego locka.

7. Admission writer:
   - event path wykonuje tylko serializację i `try_send` do bounded `sync_channel`;
   - filesystem I/O wykonuje dedicated OS thread;
   - `post_buy_admission_health_v1.json` utrwala attempts/enqueued/written/dropped/failed;
   - drop/failure nie blokuje runtime, ale oblewa evidence gate.

8. Admission reconciliation:
   - join jest dwukierunkowy: `PositionOpened -> admission` i `monitoring_registered -> PositionOpened`;
   - analyzer wymaga first HET comparison nie później niż 2 monitor ticks po `monitoring_registered`.

9. Per-run floors:
   - Gate 5 zawiera minimalną jakość per validation run, nie tylko globalną agregację.

## D5. Konsekwencje

Po zmianie PR #73 nadal jest prerequisite’em, a nie cutoverem.

Runtime:

- V1 pozostaje jedynym lifecycle ownerem;
- V2/HET evidence nie tworzy proposal, apply ani terminal truth;
- admission writer failure/degradation nie blokuje handoffu ani capacity release;
- admission evidence staje się audytowalnym artifactem z własnym denominator health.

Analyzer:

- promotion gate może przejść tylko, gdy wszystkie requested gates indywidualnie przejdą swoje sample/economic/coverage checks;
- jedna pozycja liczy się globalnie tylko raz;
- brakujące dane i późne/brakujące first-HET są naruszeniami kontraktu;
- calibration/relabel/mixed-manifest paths pozostają fail-closed.

## D6. Implementacja

Zmiany obejmują:

- `scripts/het_pm_v2_promotion_gate_v1.py`
  - schema/tool/criteria v3;
  - gate-specific economics;
  - combined-policy outcome;
  - source-reconstructing manifest validation;
  - route allowlist;
  - fail-closed economic joins;
  - per-run stability floors;
  - admission health reconciliation.

- `PLANS/DO_REALIZACJI/HET_PM_V2_PROMOTION_CRITERIA_V1.json`
  - criteria v3;
  - frozen provenance fields;
  - relaxed sample thresholds zgodnie z operacyjną dyspozycją;
  - fail-closed zero-tolerance dla evidence loss.

- `scripts/start_selector_lifecycle_run.py`
  - required `--run-role`;
  - required `--launch-cohort-id`;
  - run role persisted in launcher report before runtime start.

- `ghost-launcher/src/components/post_buy_runtime.rs`
  - bounded admission writer;
  - admission health artifact;
  - terminal watcher admission release przez writer, nie przez synchroniczne file I/O.

- tests:
  - Python promotion gate and launcher guard tests;
  - Rust terminal watcher admission health test.

## D7. Weryfikacja

Wykonane lokalnie:

```text
python3 scripts/test_het_pm_v2_promotion_gate_v1.py
python3 scripts/test_selector_lifecycle_run_guard.py
cargo test -p ghost-launcher components::post_buy_runtime::tests::shadow_terminal_watcher_writes_admission_terminal_release --lib
cargo fmt
```

Wynik:

- promotion gate tests: PASS;
- launcher guard tests: PASS;
- targeted Rust terminal watcher/admission test: PASS;
- format: applied.

## D8. Ryzyka i następne kroki

Ryzyka pozostające celowo poza tym PR:

- finalne prospective validation runy muszą zostać uruchomione dopiero po zamrożeniu rzeczywistego runtime commit/binary/config locka;
- aktualne r2a pozostaje calibration/diagnostic, nie final validation run;
- authority cutover wymaga osobnego PR po `promotion_gate_passed=true`.

Następne kroki:

1. zbudować finalną release binarkę po merge/akceptacji prerequisite;
2. zamrozić exact validation contract dla dwóch nowych prospective validation runów;
3. uruchomić dwa runy validation bez dalszego strojenia;
4. ocenić artifact przez source-recomputing `evaluate`/`validate`;
5. dopiero po PASS przygotować osobny cutover PR.
