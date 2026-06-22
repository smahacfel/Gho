# ADR-8D: Selector Soft Score Buy Gate

Status: IMPLEMENTED / TARGETED_TESTS_PASSED
Typ: ADR-8D / Gatekeeper selector scoring, config rollout safety, additive decision logging
Data: 2026-06-20
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: implementacja konfigurowalnego `selector_soft_score` 12-regulowego jako soft scoring / BUY gate dla testow selektora R37/R38
Poziom ryzyka: HIGH

Dotkniete moduly/pliki:
- `ghost-brain/src/config/ghost_brain_config.rs`
- `ghost-brain/src/config/mod.rs`
- `ghost-brain/ghost_brain_config.toml`
- `configs/rollout/ghost_brain_selector_dataset_sampler_r37_threshold_probe_maxwait3789_fsc_off.toml`
- `configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`
- `configs/rollout/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1.toml`
- `configs/rollout/shadow-burnin-v3-r38-threshold-probe-target50-stop50-fsc-off-r1.toml`
- `ghost-core/src/checkpoint/types.rs`
- `ghost-launcher/src/session/observation.rs`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-brain/src/oracle/reason_code.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- `docs/ADR/ADR_8D_SELECTOR_SOFT_SCORE_BUY_GATE_20260620.md`

Powiazane plany:
- `PLANS/DO_REALIZACJI/PLAN_EVIDENCE_COVERAGE_CONTRACT_CPV_TEMPORAL_BURST_20260618.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ADR-ach PR1-PR6.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zastapic probe strict hard-AND dla wskazanych metryk selektora pozytywnym scoringiem 12 regul. Kazda regula daje punkt tylko wtedy, gdy metryka jest dostepna i spelnia prog. Brak metryki nie jest liczony jako `0.0`, nie daje punktu i musi pozostac widoczny w diagnostyce.

Wymagania uzytkownika:
- `score >= 2` oznacza `candidate`.
- `score >= 3` oznacza `buy_allowed`.
- W profilach testowych R37/R38 selector ma dzialac jako BUY gate.
- Hard safety gates pozostaja przed scoringiem i nie moga byc omijane.
- CPV nadal mierzy successful-buy signer semantics, nie wszystkich signerow.
- Degraded CPV i carried-forward temporal deltas moga byc uzyte przez selector tylko jawnie, przez config i z oznaczeniem statusu.
- Wynik i lista regul maja byc logowane do JSONL dla pozniejszej analizy rankingowej.

Rzeczywisty przebieg:
- Potwierdzono, ze aktywny tor decyzji przechodzi przez `MaterializedFeatureSet`, `evaluate_policy_from_assessment()` i runtime adapter `GatekeeperBuffer::compute_decision()`.
- Dodano inert default config w bazowym `ghost_brain_config.toml`, a aktywne `buy_gate` tylko w profilach R37/R38.
- Nie zmieniano Solana execution path, shadow/live boundary, sendera, TX builderow ani ingestu.
- Nie robiono imputacji `null -> 0.0`.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: Gatekeeper flow, SSOT, DecisionLogger/replay boundary, shadow/live separation.
- `trading-systems`: hard safety vs soft selector, fail-closed policy, decyzja BUY po scoringu.
- `rust-master`: typed config, serde defaults, deterministic policy helpers.

Zaladowane/uwzglednione role specjalistyczne:
- `Gatekeeper Policy Auditor`: aktywny path decyzji i terminal verdicts.
- `SSOT Feature Materialization Guardian`: `MaterializedFeatureSet` jako zrodlo cech selektora.
- `Config Rollout Safety Reviewer`: nowe pola TOML, serde defaults, profile R37/R38.
- `Decision Logging Replay Analyst`: additive JSONL fields i reason-code taxonomy.

Nie ladowano/nie dotykano:
- `Solana Execution Path Engineer`: brak zmian w budowie, wysylce, symulacji lub potwierdzaniu transakcji.
- `Seer Ingest Event Integrity Specialist`: brak zmian w parserach, Yellowstone/Geyser i event ordering.

## 3. Opis problemu - 3W2H

What:
Poprzedni model testowania progow selektora zachowywal sie jak twardy AND: niespelnienie pojedynczego progu metryki moglo konczyc sie rejectem. To nie pozwalalo przetestowac hipotezy, ze zestaw slabszych, ale powtarzalnych sygnalow moze tworzyc dobry selector ranking.

Where:
- `GatekeeperV2Config`
- profile R37/R38
- `MaterializedFeatureSet.alpha_fingerprint`
- `GatekeeperPolicy`
- `GatekeeperDecision`
- `DecisionLogger` / `GatekeeperBuyLog`

Why it matters:
Twardy AND miesza dwie klasy zdarzen:
- token jest zly, bo nie spelnia konkretnego hard safety condition,
- token nie zdobyl punktu w jednej z wielu dodatnich regul selektora.

To falszuje dataset, bo pojedyncza metryka testowa staje sie reject reason, zamiast byc skladnikiem rankingu. Jednoczesnie nie wolno poprawiac coverage przez ciche podstawianie zer albo przez rozszerzanie CPV na inna semantyke signerow.

How observed:
Uzytkownik wskazal zestaw 12 metryk/progow i oczekiwany model:
- kazda regula daje `+1`,
- score 0-12,
- missing metric = no point,
- candidate od 2 punktow,
- BUY od 3 punktow.

How many / scale:
Zmiana dotyczy kazdej decyzji Gatekeepera, gdy `gatekeeper_v2.selector_soft_score.enabled=true`. Domyslnie w bazowym configu jest wylaczona i `policy="log_only"`.

## 4. Przyczyna zrodlowa

Root cause:
Ghost mial mechanizmy soft signals i evidence context, ale nie mial osobnego, konfigurowalnego, dodatniego scoringu selectorowego dla zestawu metryk badawczych. W efekcie progi selektora byly latwe do pomylenia z hard gates.

Konkretnie brakowalo:
- typed config sekcji `[gatekeeper_v2.selector_soft_score]`,
- walidacji score thresholds i wag,
- policy helpera liczacego score z `MaterializedFeatureSet`,
- jawnego terminal behavior `log_only | candidate_only | buy_gate`,
- typed rejects dla przypadkow `score < candidate` i `candidate <= score < buy`,
- top-level loggingu score i per-rule diagnostics.

## 5. Strategia naprawy

Przyjeta strategia:
- Dodac `SelectorSoftScoreConfig` jako serde-default, addytywny config surface.
- Utrzymac bazowy config inert: `enabled=false`, `policy="log_only"`.
- W profilach R37/R38 wlaczyc `enabled=true`, `policy="buy_gate"`, `min_candidate_score=2`, `min_buy_score=3`.
- Dla 12 regul uzyc wag konfigurowalnych, domyslnie `1`.
- Brak metryki oznaczyc jako `missing`, bez punktu i bez imputacji.
- CPV rules (`signer_cross_pool_velocity`, `cpv_other_pool_activity`) czytac z CPV evidence context i honorowac `allow_degraded_cpv`.
- Temporal delta rules czytac z temporal delta evidence i honorowac `allow_carried_temporal_deltas` oraz globalny `temporal_carried_forward_policy`.
- Wpiac selector po hard safety/core/sybil/strict policy layers, przed alpha/prosperity BUY path.
- Logowac score i per-rule diagnostics addytywnie, bez destrukcyjnej zmiany istniejacych pol.

Granice:
- Brak `null -> 0.0`.
- Brak future-fill.
- Brak zmiany CPV na all-signers.
- Brak bypassu hard safety gates.
- Brak zmian w live execution.
- Brak zmian w ingest.
- Brak dataset-builder imputacji.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: config selector soft score
- Plik: `ghost-brain/src/config/ghost_brain_config.rs`
- Dodano `SelectorSoftScorePolicy`:
  - `log_only`
  - `candidate_only`
  - `buy_gate`
- Dodano `SelectorSoftScoreMissingPolicy::no_point`.
- Dodano `SelectorSoftScoreConfig` z progami, wagami, `min_candidate_score`, `min_buy_score`, `allow_degraded_cpv`, `allow_carried_temporal_deltas`.
- Dodano `max_score()` i `validate()`.
- Dodano field `GatekeeperV2Config.selector_soft_score` z `#[serde(default)]`.

Zmiana 2: TOML surface
- Pliki:
  - `ghost-brain/ghost_brain_config.toml`
  - R37/R38 rollout sampler configs
  - R37/R38 rollout wrapper configs
- Bazowy config: selector wylaczony, `log_only`.
- R37/R38: selector wlaczony jako `buy_gate`, score candidate/buy `2/3`, strict metric threshold gate wylaczony dla tych profili.

Zmiana 3: missing MFS field
- Pliki:
  - `ghost-core/src/checkpoint/types.rs`
  - `ghost-launcher/src/session/observation.rs`
- Dodano `avg_cpi_depth_50tx: Option<f64>` do `AlphaFingerprintFeatures`.
- Materializacja przenosi `fingerprint.avg_cpi_depth_50tx` do snapshotu, zeby selector nie czytal konkurencyjnego runtime source.

Zmiana 4: policy helper i terminal verdicts
- Plik: `ghost-launcher/src/components/gatekeeper_policy.rs`
- Dodano `compute_selector_soft_score()`, ktory liczy 12 regul:
  - `jito_tip_intensity < 0.4132`
  - `unique_ratio >= 0.243`
  - `cpv_other_pool_activity < 2.2111`
  - `max_single_sell_impact_pct_observed < 31.4`
  - `signer_cross_pool_velocity < 0.6631`
  - `hhi < 0.2300`
  - `avg_cpi_depth_50tx < 2.84`
  - `top3_volume_pct < 0.749`
  - `delta_jito_tip_intensity_1s_to_2s < 0.0931`
  - `same_ms_tx_ratio >= 0.049`
  - `interval_cv >= 0.904`
  - `delta_jito_tip_intensity_1s_to_3s < 0.1615`
- Dodano `selector_soft_score_terminal_reject()`:
  - disabled/log_only: brak zmiany verdictu,
  - candidate_only: reject tylko ponizej candidate,
  - buy_gate: reject ponizej candidate albo ponizej buy.

Zmiana 5: runtime adapter
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- Dodano `SelectorSoftScoreDiagnostics` i `SelectorSoftScoreRuleDiagnostic`.
- Dodano verdicts:
  - `REJECT_SELECTOR_NOT_CANDIDATE`
  - `REJECT_SELECTOR_BELOW_BUY`
- Wpięto selector do `GatekeeperDecision`, gate trace, terminal gate mapping, first-kill diagnostics i buy log mapping.

Zmiana 6: reason-code taxonomy
- Plik: `ghost-brain/src/oracle/reason_code.rs`
- Dodano typed reason codes:
  - `RejectSelectorNotCandidate`
  - `RejectSelectorBelowBuy`
- Dodano mapping z verdict strings.

Zmiana 7: additive DecisionLogger fields
- Plik: `ghost-brain/src/oracle/decision_logger.rs`
- Podbito schema version do `30`.
- Dodano top-level fields:
  - `selector_soft_score_enabled`
  - `selector_soft_score_policy`
  - `selector_soft_score_missing_policy`
  - `selector_soft_score`
  - `selector_soft_score_max`
  - `selector_soft_score_present_rules`
  - `selector_soft_score_passed_rules`
  - `selector_soft_score_missing_rules`
  - `selector_soft_score_min_candidate`
  - `selector_soft_score_min_buy`
  - `selector_soft_score_candidate_passed`
  - `selector_soft_score_buy_passed`
  - `selector_soft_score_rules`

Zmiana 8: tests
- Dodano/rozszerzono testy dla:
  - score 3/12 => BUY przy `buy_gate`,
  - score 2/12 => candidate, ale reject ponizej buy,
  - score 1/12 => reject not candidate,
  - missing metric => no point, nie zero,
  - degraded CPV wymaga `allow_degraded_cpv`,
  - carried-forward temporal delta wymaga `allow_carried_temporal_deltas` i kompatybilnej globalnej temporal policy,
  - R37/R38 profile fields,
  - reason-code mapping,
  - feature builder compatibility.

## 7. Walidacja dzialan naprawczych

### Targeted validation

| Walidacja | Komenda | Wynik | Status |
|---|---|---|---|
| Format touched Rust packages | `cargo fmt --package ghost-brain --package ghost-core --package ghost-launcher` | passed | PASS |
| Selector soft score policy tests | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-launcher selector_soft_score --lib` | 6 passed | PASS |
| Config enum deserialization | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-brain test_gatekeeper_v2_evidence_policy_enums_deserialize_all_values --lib` | 1 passed | PASS |
| Base config parse | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-brain parse_gatekeeper_v25_config --lib` | 1 passed | PASS |
| R37/R38 rollout profile config | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-brain gatekeeper_v2_r37_and_r38_profiles_define_pr1_evidence_foundation_fields --lib` | 1 passed | PASS |
| Reason code taxonomy | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-brain reason_code --lib` | 11 passed | PASS |
| Core feature builder compatibility | `RUSTFLAGS=-Awarnings cargo test -q -p ghost-core --test feature_builder_tests` | 5 passed | PASS |
| Whitespace / patch hygiene | `git diff --check` | clean | PASS |

Uwaga:
Testy z `RUSTFLAGS=-Awarnings` wyciszaja istniejace warningi repo, zeby wynik byl jednoznaczny. Nie oznacza to, ze warningi zniknely z projektu.

### Runtime/log proof status

Nie uruchamiano nowego runtime runa w ramach tego ADR. Wymagany nastepny proof:
- R37/R38 po rebuildzie,
- potwierdzenie `log_schema_version=30`,
- potwierdzenie top-level `selector_soft_score*`,
- potwierdzenie `selector_soft_score_rules`,
- rozklad verdictow `REJECT_SELECTOR_NOT_CANDIDATE`, `REJECT_SELECTOR_BELOW_BUY`, `BUY`,
- potwierdzenie, ze hard safety rejects nadal wystepuja przed selector gate.

## 8. Kontrakty zachowane

SSOT:
- Selector czyta z `GatekeeperAssessment` / `MaterializedFeatureSet`, nie z konkurencyjnego mutable runtime state.

Hard safety:
- Selector jest pozytywnym scoringiem po warstwach hard/core/sybil/strict policy. Nie omija hard filters.

Missing semantics:
- Missing/non-finite feature nie daje punktu i jest logowane per-rule. Nie ma `null -> 0.0`.

CPV semantics:
- CPV pozostaje successful-buy signer metric. Degraded low-sample CPV moze dac punkt tylko przy `allow_degraded_cpv=true` i nadal nosi status degraded.

Temporal carry-forward:
- Carried-forward delta moze dac punkt tylko przy `allow_carried_temporal_deltas=true` oraz globalnej polityce `use_for_selector_only` albo `use_in_policy`, z kontrola staleness.

Config compatibility:
- Nowe pola maja serde defaults. Stare configi powinny dalej sie ladowac.

Logging/replay:
- Nowe pola JSONL sa addytywne.
- Per-rule diagnostics zachowuja threshold, value, status, reason i points.

Shadow/live:
- Nie wlaczono live execution.
- Nie zmieniono sendera, builderow, blockhash, retries ani confirmation.

## 9. Ryzyka regresji i zabezpieczenia

Ryzyko 1: selector przypadkowo staje sie hard safety bypass.
- Kiedy: jesli policy order zostanie pozniej przestawiony przed hard/core gates.
- Zabezpieczenie: selector terminal reject jest wpięty dopiero po istniejacych reject layers; tests pokrywaja selector behavior, ale runtime proof musi sprawdzic gate trace.

Ryzyko 2: missing metrics beda interpretowane jako 0.
- Kiedy: jesli dataset builder albo przyszly logger zinterpretuje brak `value` jako numeric zero.
- Zabezpieczenie: per-rule `status="missing"`, `value=null`, `points=0`; brak imputacji w Rust helperze.

Ryzyko 3: degraded CPV zostanie uzyty jak clean.
- Kiedy: jesli wlaczy sie `allow_degraded_cpv` bez analizy albo downstream zignoruje `status`.
- Zabezpieczenie: rule status pozostaje `degraded_low_sample`; config context loguje allow flag.

Ryzyko 4: carried-forward delta bedzie traktowana jak swiezo observed.
- Kiedy: jesli downstream bierze tylko liczbe i ignoruje status/source.
- Zabezpieczenie: selector rule status odroznia `carried_forward`; policy wymaga explicit allow i kompatybilnej globalnej temporal policy.

Ryzyko 5: strict hard AND zostanie przypadkowo w tym samym profilu.
- Kiedy: jesli R37/R38 maja jednoczesnie `strict_metric_threshold_gate_enabled=true` i selector `buy_gate`.
- Zabezpieczenie: profile test sprawdza `strict_metric_threshold_gate_enabled=false` dla R37/R38 selector profiles.

Ryzyko 6: replay/schema consumer nie rozpozna nowego JSONL shape.
- Kiedy: downstream oczekuje starej schema i ignoruje additive fields albo wymaga starego numeru.
- Zabezpieczenie: schema bumped do 30; pola addytywne i `serde(default)` po stronie structu.

Ryzyko 7: candidate_only zostanie pomylone z buy_gate.
- Kiedy: ktos ustawi `policy="candidate_only"` oczekujac BUY threshold.
- Zabezpieczenie: enum policy jest jawne; R37/R38 testuja `buy_gate`; ADR opisuje semantyke.

## 10. Residuals / dalsze kroki

Pozostaje do wykonania poza tym ADR:
- fresh runtime proof na R37/R38 po rebuildzie,
- analiza rozkladu score i top rules na wiekszej probce,
- potwierdzenie, czy `score >= 3` daje sensowny BUY rate,
- ewentualne dostrojenie wag/progow po analizie datasetu,
- aktualizacja analizatorow offline, jesli maja bezposrednio rankingowac `selector_soft_score_rules`.

## 11. Decyzja

Implementacja selector soft-score zostaje przyjeta jako addytywna, konfigurowalna warstwa pozytywnego scoringu. Nie zastępuje hard safety gates. W profilach testowych R37/R38 moze pelnic role BUY gate, ale runtime acceptance wymaga swiezego runa i audytu artefaktow JSONL.
