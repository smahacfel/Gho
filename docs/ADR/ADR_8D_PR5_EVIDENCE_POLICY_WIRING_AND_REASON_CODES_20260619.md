# ADR-8D: PR5 Evidence Policy Wiring and Reason Codes

Status: IMPLEMENTED / TARGETED_RUNTIME_SMOKE_VERIFIED
Typ: ADR-8D / Gatekeeper policy wiring, evidence-quality policy, reason-code auditability
Data: 2026-06-19
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR update time
Zakres: PR5 z planu evidence coverage contract; strict metric missing policy, CPV low-sample policy, temporal carried-forward policy annotation, reason-chain separation, additive buy-log evidence policy context
Poziom ryzyka: HIGH

Dotkniete moduly/pliki:
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-brain/src/oracle/reason_code.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- `docs/ADR/ADR_8D_PR5_EVIDENCE_POLICY_WIRING_AND_REASON_CODES_20260619.md`

Powiazane plany:
- `PLANS/DO_REALIZACJI/PLAN_EVIDENCE_COVERAGE_CONTRACT_CPV_TEMPORAL_BURST_20260618.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ADR-ach PR1/PR2/PR3/PR4.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zrealizowac PR5, czyli podlaczyc pola konfiguracyjne z PR1 i evidence surface z PR2/PR3/PR4 do aktywnego Gatekeeper policy path w sposob fail-closed, jawny i audytowalny.

Rzeczywisty przebieg:
- Potwierdzono, ze active policy path przechodzi przez `evaluate_policy_from_assessment()` i `GatekeeperBuffer::compute_decision()`, a kanonicznym snapshotem jest `MaterializedFeatureSet`.
- Potwierdzono, ze PR1 fields i PR2/PR3/PR4 evidence shell sa juz obecne w dirty working tree.
- Nie cofano ani nie przepisywano szerokich zmian z PR1-PR4.
- PR5 zostal wykonany jako policy wiring i logging/audit extension, bez zmian w materializacji temporal deltas, bez zmian w CPV rolling index semantics, bez zmian w Solana execution path.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: Gatekeeper flow, SSOT, DecisionLogger/replay boundary, shadow/live separation.
- `rust-master`: deterministic Rust implementation, typed helpers, conservative enum handling.
- `trading-systems`: rozdzielenie wartosci liczbowej, missing evidence, degraded evidence i carried-forward evidence.

Zaladowane dokumenty/specjalistyczne instrukcje:
- Repo-local `AGENTS.md`
- `.agents/skills/ghost-execution/SKILL.md`
- `.agents/skills/rust-master/SKILL.md`
- `.agents/skills/trading-systems/SKILL.md`
- `docs/agents/gatekeeper-policy-auditor.md`
- `docs/agents/config-rollout-safety-reviewer.md`
- `docs/agents/decision-logging-replay-analyst.md`
- `docs/agents/ssot-feature-materialization-guardian.md`

Nie ladowano dokumentow:
- `solana-execution-path-engineer`: PR5 nie dotyka sendera, TX builderow, blockhash, retry ani confirmation.
- `seer-ingest-event-integrity-specialist`: PR5 nie dotyka ingestu, parserow, Yellowstone/Geyser ani event ordering.

## 3. Opis problemu - 3W2H

What:
Po PR1-PR4 system mial surface dla evidence policy, ale konfiguracja nie byla konsekwentnie interpretowana w policy path. Istnialo ryzyko, ze downstream i Gatekeeper nie odroznia:
- wartosci policzonej i rzeczywistej,
- `null` / braku evidence,
- degraded low-sample evidence,
- carried-forward evidence.

Where:
- `evaluate_policy_from_assessment()`
- `GatekeeperBuffer::compute_decision()`
- timeout decision builders
- `GatekeeperBuyLog`
- `GatekeeperReasonCode`
- `reason_chain` w durable JSONL

Why it matters:
Bez jawnego policy wiring mozna uzyskac ladniejsze coverage procenty kosztem prawdy decyzyjnej. Najgorsze regresje to:
- `null` zamieniony w `0.0`,
- degraded CPV potraktowane jako clean CPV,
- carried-forward delta potraktowana jako swiezo obserwowana,
- missing runtime coverage potraktowany jako normalny sygnal rynkowy,
- dwa runy z roznymi policy configami nierozroznialne w replay.

How observed:
Runtime review poprzednich runow pokazal null/missing/degraded przypadki dla metryk takich jak `jito_tip_intensity`, `flipper_presence_ratio`, `signer_cross_pool_velocity`, CPV i temporal deltas. Uzytkownik zdecydowal, ze system ma deklarowac brak danych i jakosc evidence, a nie cicho malowac wszystko na liczby.

How many / scale:
Zmiana dotyczy kazdej decyzji Gatekeepera, ale aktywny hard-fail behavior jest kontrolowany przez config fields z PR1. Nie zmienia shadow/live boundary.

## 4. Przyczyna zrodlowa

Root cause:
Evidence policy fields byly w configu, a evidence context w snapshotach/logach, ale nie bylo jednego wspolnego interpretatora strict metric evidence w aktywnym Gatekeeper path.

Konkretnie:
- missing strict metrics nie mialy jednolitej polityki `hard_fail | skip | degraded_allowed`,
- CPV low-sample evidence nie mialo jawnego mappingu `hard_fail | use_degraded | reason_only`,
- carried-forward temporal evidence nie bylo oddzielnie opisane w reason chain zgodnie z `log_only | use_for_selector_only | use_in_policy`,
- brakowalo typed reason code dla strict metric threshold hard-fail,
- buy log nie niosl wystarczajacego evidence policy context do pozniejszego replay/audytu.

## 5. Strategia naprawy

Przyjeta strategia:
- Zachowac `MaterializedFeatureSet` jako SSOT; policy helper czyta z assessment/snapshotu, nie z konkurencyjnego mutable state.
- Dodac jeden strict/evidence policy evaluator i uzyc go w aktywnym policy path oraz legacy/buffer decision path, zeby uniknac rozjazdu.
- Utrzymac fail-closed defaults: missing/degraded nie staje sie liczba.
- Dla `strict_metric_missing_policy=skip` nie robic hard fail, ale emitowac `metric_skipped_missing` i `strict_metric_pass=false`.
- Dla `strict_metric_missing_policy=degraded_allowed` dopuszczac tylko jawne degraded evidence tam, gdzie metryka ma evidence status; czysty `null` pozostaje hard-fail.
- Dla CPV zostac przy successful-buy signer semantics. Nie liczyc wszystkich signerow, failed tx ani sell-only wallets.
- Dla `cpv_low_sample_policy=reason_only` logowac low-sample reason bez threshold-pass.
- Dla `cpv_low_sample_policy=use_degraded` wymagac dodatkowo `cpv_allow_degraded_in_strict_policy=true`; degraded CPV nadal nosi status degraded i moze przegrac threshold.
- Dla temporal carried-forward values nie zmieniac Gatekeeper verdict przy `log_only` i `use_for_selector_only`.
- Dla `use_in_policy` oznaczac tylko allowlisted metric classes i staleness; obecnie PR5 nie dodaje nowych temporal thresholdow, wiec nie powstaje ukryty verdict change.
- Dodac additive `evidence_policy_context` do buy loga i podbic schema version.

Granice:
- Brak `null -> 0.0`.
- Brak future-fill.
- Brak rozszerzania CPV na wszystkich signerow.
- Brak zmian w CPV rolling index.
- Brak zmian w temporal materialization.
- Brak zmian w Seer/ingest.
- Brak zmian w Solana execution, shadow dispatch, DirectBuyBuilder, senderze lub confirmation.
- Brak dataset-builder imputacji.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: strict metric policy evaluator
- Plik: `ghost-launcher/src/components/gatekeeper_policy.rs`
- Dodano wspolny evaluator strict/evidence policy, ktory sprawdza:
  - `total_tx_evaluated`,
  - `unique_signers_evaluated`,
  - `buy_count`,
  - canonical `burst_ratio`,
  - `flipper_presence_ratio`,
  - `jito_tip_intensity`,
  - `signer_cross_pool_velocity` / CPV.
- Zwraca hard-fail reason oraz audit annotations do `reason_chain`.

Zmiana 2: strict metric missing policy
- Plik: `ghost-launcher/src/components/gatekeeper_policy.rs`
- `hard_fail`: missing strict metric daje `RejectHardFail`.
- `skip`: missing metric daje `metric_skipped_missing=<metric>` i `strict_metric_pass=false`, bez threshold pass.
- `degraded_allowed`: czysty `null` nie jest imputowany i nadal fail-closed; degraded evidence moze byc interpretowane tylko jawnie przez metryke, ktora ma evidence status.

Zmiana 3: CPV low-sample policy
- Plik: `ghost-launcher/src/components/gatekeeper_policy.rs`
- `hard_fail`: degraded/low-sample CPV odrzuca jako `strict_metric_degraded_not_allowed=signer_cross_pool_velocity`.
- `reason_only`: emituje `cpv_low_sample_reason_only=signer_cross_pool_velocity` oraz `strict_metric_pass=false`, bez uzycia wartosci jako pass.
- `use_degraded`: wymaga `cpv_allow_degraded_in_strict_policy=true`; wartosc jest threshold-evaluated z `evidence_status=degraded_low_sample`, nie jako clean.

Zmiana 4: temporal carried-forward policy annotations
- Plik: `ghost-launcher/src/components/gatekeeper_policy.rs`
- `log_only`: emituje `temporal_carried_forward_log_only=<metric>` i nie zmienia verdict.
- `use_for_selector_only`: emituje `temporal_carried_forward_selector_only=<metric>` i nie zmienia verdict.
- `use_in_policy`: emituje `temporal_carried_forward_policy_allowed=<metric>` tylko dla allowlisted metric class i staleness pod limitem; w innym przypadku `temporal_carried_forward_not_allowed=<metric>`.
- Obecnie nie dodano nowego Gatekeeper threshold consumer dla temporal deltas, wiec PR5 nie tworzy ukrytego temporal verdict change.

Zmiana 5: typed hard-fail reason code
- Plik: `ghost-brain/src/oracle/reason_code.rs`
- Dodano `GatekeeperReasonCode::HardFailStrictMetricThreshold`.
- Dodano mapping z hard-fail reason `StrictMetricThreshold`.
- Dodano testy roundtrip/string form dla nowego reason code.

Zmiana 6: policy path wiring
- Pliki:
  - `ghost-launcher/src/components/gatekeeper_policy.rs`
  - `ghost-launcher/src/components/gatekeeper.rs`
- `evaluate_hard_filters_from_assessment()` zwraca strict metric hard fail przy wlaczonym strict gate.
- `evaluate_policy_from_assessment()` i timeout decision buildery dopinaja evidence annotations do reason chain.
- `GatekeeperBuffer::compute_decision()` uzywa tego samego strict helpera, zeby buffer path nie mial innej semantyki niz policy assessment path.
- Timeout branch moze zwrocic `RejectHardFail`, jesli materialized hard filter/strict metric hard fail jest juz znany; w przeciwnym razie timeout pozostaje timeoutem z evidence annotations.

Zmiana 7: additive evidence policy context in buy log
- Plik: `ghost-brain/src/oracle/decision_logger.rs`
- Dodano `GatekeeperBuyLog.evidence_policy_context: Option<serde_json::Value>` z `serde(default, skip_serializing_if = "Option::is_none")`.
- Podbito `GATEKEEPER_BUY_LOG_SCHEMA_VERSION` do `28`, bo JSONL shape niesie nowy replay/audit context.

Zmiana 8: evidence policy context mapper
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- `GatekeeperAssessment::to_buy_log()` emituje `evidence_policy_context`, gdy `config.emit_evidence_policy_context=true`.
- Kontekst obejmuje m.in.:
  - `strict_metric_threshold_gate_enabled`,
  - `strict_metric_missing_policy`,
  - `cpv_low_sample_policy`,
  - CPV clean/degraded thresholds,
  - `cpv_allow_degraded_in_strict_policy`,
  - `temporal_carried_forward_policy`,
  - temporal carry-forward config flags,
  - `top_level_features_from_materialized_ssot`.

Zmiana 9: test coverage
- Plik: `ghost-launcher/src/components/gatekeeper_policy.rs`
- Dodano testy dla:
  - missing jito/flipper/CPV hard-fail,
  - count shortfall value failure,
  - `strict_metric_missing_policy=skip`,
  - `strict_metric_missing_policy=degraded_allowed` bez imputacji null,
  - wszystkich trzech `cpv_low_sample_policy`,
  - `cpv_allow_degraded_in_strict_policy`,
  - `temporal_carried_forward_policy` log/selector/use-in-policy,
  - emisji `evidence_policy_context` w buy logu.

## 7. Walidacja dzialan naprawczych

### Targeted validation

| Walidacja | Komenda | Wynik | Status |
|---|---|---|---|
| Format touched Rust packages | `cargo fmt --package ghost-launcher --package ghost-brain` | passed | PASS |
| Evidence policy context log mapping | `cargo test -q -p ghost-launcher evidence_policy_context --lib` | 1 passed | PASS |
| Strict metric policy tests | `cargo test -q -p ghost-launcher strict_metric --lib` | 4 passed | PASS |
| CPV low-sample policy tests | `cargo test -q -p ghost-launcher cpv_low_sample --lib` | 4 passed | PASS |
| Temporal carried-forward policy tests | `cargo test -q -p ghost-launcher temporal_carried_forward --lib` | 3 passed | PASS |
| Reason code tests / schema compile | `cargo test -q -p ghost-brain reason_code --lib` | 11 passed | PASS |

Uwaga:
Test commands emituja wiele istniejacych repo warnings dotyczacych deprecated/unused paths. W testach PR5 nie bylo failure.

### Runtime/log proof status

Code/static proof:
- Active policy helper czyta z `GatekeeperAssessment` i `MaterializedFeatureSet`.
- Hard fail strict metric ma typed reason code `HARD_FAIL_STRICT_METRIC_THRESHOLD`.
- Reason chain rozroznia:
  - `strict_metric_missing`,
  - `metric_skipped_missing`,
  - `strict_metric_degraded_not_allowed`,
  - `cpv_low_sample_reason_only`,
  - `strict_metric_value_failure`,
  - `temporal_carried_forward_*`.
- Buy log niesie `evidence_policy_context` przy wlaczonym configu.

Fresh runtime smoke:
- Scope: `shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1`.
- Tmux session: `r37-pr5-cpv-proof-20260619`.
- Lifecycle report: `reports/selector/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260619T134238Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json`.
- Launcher guard: `status=PASS`, `claim=SELECTOR_EVENT_CANARY_RUN_STARTED_ZERO_BUY_LIFECYCLE_ALLOWED`.
- Fresh decision files use hash `45137ae410c1ab231b457abed6a34f99b4086136f912e6de64c7dd703d6850d8`.
- Important audit note: JSONL files were append-only. Lines before fresh schema-28 segment are older records from prior runs and must not be mixed into PR5 proof.

Fresh schema-28 segment audit:

| Gate | legacy_live | v25_shadow | Status |
|---|---:|---:|---|
| Fresh schema-28 records | 2011 | 850 | INFO |
| First fresh line | 3000 | 3000 | INFO |
| Invalid JSON in fresh segment | 0 | 0 | PASS |
| `v3_materialized_feature_snapshot` present | 2011/2011 | 850/850 | PASS |
| `evidence_policy_context` present | 2011/2011 | 850/850 | PASS |
| Evidence policy context variants | 1 | 1 | PASS |
| Expected policy values matched | 2011/2011 | 850/850 | PASS |
| Top-level `burst_ratio` vs embedded canonical mismatch | 0 | 0 | PASS |
| Top-level nullable vectors vs embedded decision series mismatch | 0 | 0 | PASS |
| Core `delta_*` top-level vs embedded mismatch | 0 | 0 | PASS |
| `rate_*` top-level vs embedded mismatch | 0 | 0 | PASS |
| CPV top-level vs embedded mismatch | 0 | 0 | PASS |
| CPV degraded top-level missing despite `cpv_emit_degraded_low_sample=true` | 0 | 0 | PASS |
| CPV insufficient/unavailable value emitted top-level | 0 | 0 | PASS |
| Carried-forward delta evidence missing top-level numeric value | 0 | 0 | PASS |
| `not_allowed` temporal evidence emitted as top-level value | 0 | 0 | PASS |

Observed fresh-segment evidence:
- `legacy_live` CPV quality: `clean=690`, `degraded_low_sample=549`, `insufficient_sample=772`.
- `v25_shadow` CPV quality: `clean=575`, `degraded_low_sample=275`.
- `legacy_live` temporal status: `clean=812`, `insufficient_sample=617`, `degraded=453`, `unavailable=129`.
- `v25_shadow` temporal status: `clean=685`, `insufficient_sample=15`, `degraded=150`.
- `legacy_live` reason hits: `strict_metric_value_failure=1684`, `strict_metric_missing=119`, `temporal_carried_forward_log_only=55`.
- `v25_shadow` reason hits: `strict_metric_value_failure=809`.
- `legacy_live` selected present/nonzero examples:
  - `delta_jito_tip_intensity_1s_to_2s`: `present=1101`, `nonzero=658`.
  - `delta_flipper_presence_ratio_1s_to_2s`: `present=1101`, `nonzero=486`.
  - `delta_signer_cross_pool_velocity_1s_to_2s`: `present=368`, `nonzero=149`.
  - `delta_buy_count_1s_to_3s`: `present=1265`, `nonzero=695`.
  - `rate_net_quote_sol_per_s_1s_to_3s`: `present=1265`, `nonzero=909`.
- `v25_shadow` selected present/nonzero examples:
  - `delta_jito_tip_intensity_1s_to_2s`: `present=798`, `nonzero=510`.
  - `delta_flipper_presence_ratio_1s_to_2s`: `present=798`, `nonzero=454`.
  - `delta_signer_cross_pool_velocity_1s_to_2s`: `present=355`, `nonzero=149`.
  - `delta_buy_count_1s_to_3s`: `present=835`, `nonzero=600`.
  - `rate_net_quote_sol_per_s_1s_to_3s`: `present=835`, `nonzero=736`.

Runtime branch not exercised:
- `rate_mcap_sol_per_s_2s_to_3s` did not appear in the fresh runtime segment (`embedded_present=0`, `top_present=0`) because no numeric mcap 2s-to-3s rate was materialized in the observed window.
- The logger/top-level mapping for `rate_mcap_sol_per_s_2s_to_3s` is covered deterministically by `test_fingerprint_metrics_map_to_buy_log_and_summary`.

Not executed in this ADR:
- Long multi-hour runtime proof on R37/R38 JSONL.
- Full workspace test suite.

## 8. Ryzyka regresji i jak zostaly ograniczone

Ryzyko 1: `null` zostaje zamienione na `0.0`.
- Kiedy: missing jito/flipper/CPV traktujemy jako liczbe.
- Mitigacja: missing helper nigdy nie imputuje; `degraded_allowed` nie oznacza "null allowed".

Ryzyko 2: degraded CPV jest traktowane jak clean CPV.
- Kiedy: low-sample CPV przechodzi threshold bez statusu.
- Mitigacja: CPV path emituje `evidence_status=degraded_low_sample`; `use_degraded` wymaga osobnej flagi `cpv_allow_degraded_in_strict_policy=true`.

Ryzyko 3: CPV coverage poprawione przez zmiane znaczenia metryki.
- Kiedy: liczymy wszystkich signerow, failed tx albo sell-only wallets.
- Mitigacja: PR5 nie zmienia CPV materialization ani sample definition; zostaje successful-buy signer semantics.

Ryzyko 4: `reason_only` zaczyna przepuszczac jako threshold pass.
- Kiedy: low-sample CPV reason-only jest traktowane jako sukces strict metric.
- Mitigacja: reason chain zawiera `strict_metric_pass=false`; test potwierdza brak threshold pass.

Ryzyko 5: temporal carry-forward zmienia verdict bez jawnej zgody.
- Kiedy: carried delta jest uzyta w policy mimo `log_only` albo `use_for_selector_only`.
- Mitigacja: te dwa tryby tylko loguja i nie zmieniaja Gatekeeper verdict; `use_in_policy` ma allowlist/staleness annotation.

Ryzyko 6: strict policy path rozjezdza sie z buffer/timeout path.
- Kiedy: `evaluate_policy_from_assessment()` i `GatekeeperBuffer::compute_decision()` uzywaja innych helperow.
- Mitigacja: buffer path wywoluje ten sam strict helper i reason code.

Ryzyko 7: replay nie wie, jaka polityka byla aktywna.
- Kiedy: config policy fields nie sa logowane w decision payload.
- Mitigacja: `evidence_policy_context` jest additive w `GatekeeperBuyLog`; schema version podbita do `28`.

Ryzyko 8: DecisionLogger schema regression.
- Kiedy: stare rekordy nie maja nowego pola.
- Mitigacja: nowe pole ma `serde(default, skip_serializing_if = "Option::is_none")`; zmiana jest addytywna.

Ryzyko 9: live/shadow boundary.
- Kiedy: policy wiring przypadkiem dotyka execution path.
- Mitigacja: PR5 nie zmienia sendera, DirectBuyBuilder, confirmation, trigger dispatch ani shadow simulation semantics.

Ryzyko 10: hidden runtime coverage artifact jako sygnal rynkowy.
- Kiedy: missing/degraded/carried evidence nie jest oznaczone.
- Mitigacja: reason chain i buy-log policy context deklaruja status/policy; brak evidence nie jest ukryty jako normalna wartosc.

## 9. Stan po zmianie

Zamkniete statycznie/testowo:
- `strict_metric_missing_policy` jest podlaczony do strict metric helpera.
- `cpv_low_sample_policy` jest podlaczony dla CPV strict metrics.
- `temporal_carried_forward_policy` jest interpretowany jako jawna policy annotation bez cichego verdict change.
- Reason chain rozroznia missing, skipped, degraded-not-allowed, reason-only, threshold value failure i carried-forward modes.
- Typed reason code dla strict metric hard-fail istnieje.
- Buy log zawiera additive `evidence_policy_context`.
- Schema version buy loga wynosi `28`.

Nie zamkniete runtime-proofowo:
- Nie wykonano jeszcze dlugiego proofu wielogodzinnego.
- Krotki R37 smoke potwierdzil schema-28/context/reason-chain/top-level parity w badanym segmencie.
- `rate_mcap_sol_per_s_2s_to_3s` nie zostal naturalnie wycwiczony runtime'owo w badanym oknie; pozostaje pokryty deterministycznym testem mapowania.

## 10. Acceptance / DoD PR5

| Kryterium | Status | Dowod |
|---|---|---|
| `strict_metric_missing_policy` wired into strict helper | PASS | `strict_metric_missing_*` tests |
| `cpv_low_sample_policy` wired | PASS | `cpv_low_sample_*` tests |
| Temporal carried-forward policy handled without hidden verdict changes | PASS | `temporal_carried_forward_*` tests |
| Missing jito/flipper/CPV hard-fail under hard_fail | PASS | `strict_metric_missing_hard_fail_rejects_jito_flipper_and_cpv` |
| Skip missing logs `metric_skipped_missing` | PASS | `strict_metric_missing_skip_is_logged_without_threshold_pass` |
| Degraded CPV reason-only logged but no threshold pass | PASS | `cpv_low_sample_policy_reason_only_logs_without_threshold_pass` |
| Degraded CPV use-degraded threshold-evaluated with degraded status | PASS | `cpv_low_sample_use_degraded_evaluates_threshold_with_degraded_status` |
| `use_degraded` gated by explicit allow flag | PASS | `cpv_low_sample_use_degraded_requires_policy_allow_flag` |
| Reason code typed | PASS | `reason_code` tests |
| Evidence policy context logged | PASS | `evidence_policy_context_is_emitted_in_buy_log_when_enabled` |
| Fresh schema-28 runtime segment carries evidence policy context | PASS | R37 `45137ae...`, schema-28 segment from line 3000 |
| Top-level vs embedded burst/vector/delta/rate parity in fresh segment | PASS | R37 runtime audit, 0 mismatches |
| CPV degraded low-sample visible without clean promotion | PASS | R37 runtime audit, 549 legacy / 275 v25 degraded samples, 0 mismatches |
| `not_allowed` and carried-forward evidence not silently imputed | PASS | R37 runtime audit, 0 not-allowed-with-value and 0 carried-value-missing |

## 11. Decyzja

PR5 zostaje zaimplementowany jako fail-closed, config-driven policy wiring.

Najwazniejsza decyzja semantyczna:
System nie poprawia coverage przez falszowanie danych. Missing pozostaje missing, degraded pozostaje degraded, carried-forward pozostaje carried-forward. Policy moze byc konfigurowalna, ale evidence status musi zostac widoczny w reason chain i log payload.

## 12. Follow-up

Nastepny krok po targeted runtime smoke:
- zostawic albo uruchomic dluzszy proof runtime, jesli chcemy zamknac formalnie rzadsze branch cases;
- sprawdzic w dluzszym oknie, czy naturalnie wystapi `rate_mcap_sol_per_s_2s_to_3s`;
- nie mieszac starych appendowanych rekordow bez schema 28 z nowym segmentem PR5.
