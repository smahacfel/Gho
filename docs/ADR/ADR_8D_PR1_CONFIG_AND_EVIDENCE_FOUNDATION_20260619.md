# ADR-8D: PR1 Config and Evidence Foundation for CPV and temporal burst evidence

Status: IMPLEMENTED / STATIC_VALIDATION_COMPLETED
Typ: ADR-8D / config and additive evidence foundation
Data: 2026-06-19
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR update time
Zakres: PR1 foundation dla strict missing / CPV low-sample / temporal carry-forward config surface oraz additive evidence shell types; bez policy wiring i bez zmiany aktywnego verdict behavior
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-brain/src/config/ghost_brain_config.rs`
- `ghost-brain/src/config/mod.rs`
- `ghost-brain/ghost_brain_config.toml`
- `configs/rollout/ghost_brain_selector_dataset_sampler_r37_threshold_probe_maxwait3789_fsc_off.toml`
- `configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`
- `ghost-core/src/checkpoint/types.rs`
- `ghost-core/src/checkpoint/mod.rs`
- `ghost-core/tests/feature_builder_tests.rs`

Powiazane plany:
- `PLANS/DO_REALIZACJI/PLAN_EVIDENCE_COVERAGE_CONTRACT_CPV_TEMPORAL_BURST_20260618.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje istniejacy lokalny format ADR-8D uzyty juz w repo.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zrealizowac PR1 z planu evidence coverage contract, czyli przygotowac kompatybilny surface konfiguracyjny i evidence foundation dla CPV oraz temporal burst semantics, bez podpinania nowych wartosci do aktywnego scoringu lub typed verdict flow.

Rzeczywisty przebieg:
- Potwierdzono, ze planowe PR1 dotyczy dwoch warstw: `GatekeeperV2Config` oraz additive evidence/replay shell.
- Potwierdzono, ze decyzje JSONL juz zawieraja `gatekeeper_v2_config_payload`, wiec PR1 nie musi zmieniac `ghost-launcher`, aby zachowac policy context w logach.
- Potwierdzono, ze `ghost-core` nie moze zalezec od enumow z `ghost-brain`, wiec foundation evidence musi pozostac crate-local i neutralny wobec policy enums.
- Zakres utrzymano scisle additive: bez zmian w aktywnym Gatekeeper scoring, bez zmian BUY/REJECT/TIMEOUT, bez zmian execution/live path.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-coordination-risk-foundations`: routing i in-scope/out-of-scope dla fundamentow config/evidence.
- `ghost-execution`: SSOT, replay, shadow/live boundary i additive-only semantics.
- `rust-master`: lokalna implementacja Rust, serde defaults, testy i kompatybilnosc wsteczna.

Zaladowane dokumenty specjalistyczne:
- `docs/agents/config-rollout-safety-reviewer.md`
- `docs/agents/decision-logging-replay-analyst.md`

Powod:
PR1 dotyka config compatibility, rollout safety i audit/replay evidence, ale nie zmienia samej semantyki policy evaluation. Nie bylo potrzeby ladowania dokumentu Gatekeeper Policy Auditor, bo ten PR nie zmienia kolejnosci gates ani naliczania punktow.

## 3. Opis problemu - 3W2H

What:
Brakowalo stabilnego, wstecznie kompatybilnego kontraktu dla:
- strict missing metric behavior,
- CPV low-sample handling,
- temporal carried-forward semantics,
- evidence quality/source metadata potrzebnych pod kolejne PR-y planu.

Where:
- `GatekeeperV2Config`
- bazowy `ghost_brain_config.toml`
- rollout profiles R37 i R38
- evidence/checkpoint shared types
- serde/profile test surface

Why it matters:
Bez PR1 kolejne etapy planu musialyby:
- dopisywac policy behavior bez jawnych config flags,
- dopisywac evidence semantics bez stable shell types,
- ryzykowac drift miedzy config, runtime proof i replay evidence.

How observed:
Plan explicite rozdziela PR1 jako foundation-only. W kodzie brakowalo enum policy surface i brakowalo minimalnych additive evidence shells dla CPV/temporal quality/source, ale nie bylo jeszcze podstaw do legalnego policy/runtime wiring.

How many / scale:
Zmiana obejmuje aktywny config surface i shared evidence types, ale celowo nie dotyka aktywnego policy scoring path ani execution handoff.

## 4. Przyczyna zrodlowa

Root cause:
Evidence coverage contract byl opisany w planie, ale nie byl jeszcze zmaterializowany jako:
- stable serde enums z domyslna polityka fail-closed,
- additive config fields w `GatekeeperV2Config`,
- rollout profile fields dla planowanych wariantow R37/R38,
- shared evidence shell types do pozniejszego wypelniania przez runtime/materialization.

Dodatkowy constraint:
`ghost-core` i `ghost-brain` maja rozne odpowiedzialnosci crate-level. Nie nalezalo mieszac enum policy z shared evidence types, bo to zlamaloby aktualny boundary odpowiedzialnosci.

## 5. Strategia naprawy

Przyjeta strategia:
- Dodac nowe enumy i pola config z `#[serde(default)]` oraz fail-closed defaultami.
- Dodac pola do bazowego configu i do rollout profiles R37/R38 zgodnie z intencja planu.
- Dodac neutralne evidence shell types do `ghost-core`, bez podpinania ich do scoringu.
- Oprzec policy-context proof o istniejacy `gatekeeper_v2_config_payload`, bez zmian w runtime/logging path.

Granice:
- Brak zmian scoringu Gatekeeper V2/V2.5.
- Brak zmian typed verdict taxonomy.
- Brak zmian shadow/live execution behavior.
- Brak nowych top-level log row types.
- Brak zmian w `ghost-launcher`.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: nowe enumy polityk PR1
- Plik: `ghost-brain/src/config/ghost_brain_config.rs`
- Dodano:
  - `StrictMetricMissingPolicy`
  - `CpvLowSamplePolicy`
  - `TemporalCarriedForwardPolicy`
- Wszystkie enumy sa `snake_case` dla TOML/JSONL i maja jawne defaulty.

Zmiana 2: rozszerzenie `GatekeeperV2Config`
- Plik: `ghost-brain/src/config/ghost_brain_config.rs`
- Dodano pola:
  - `strict_metric_threshold_gate_enabled`
  - `strict_metric_missing_policy`
  - `cpv_low_sample_policy`
  - `cpv_min_successful_buy_signers_clean`
  - `cpv_min_successful_buy_signers_degraded`
  - `cpv_emit_degraded_low_sample`
  - `cpv_allow_degraded_in_strict_policy`
  - `temporal_carried_forward_policy`
  - `temporal_carry_forward_enabled`
  - `temporal_carry_forward_max_staleness_ms`
  - `temporal_carry_forward_event_counters_enabled`
  - `temporal_carry_forward_state_metrics_enabled`
  - `temporal_carry_forward_ratio_metrics_enabled`
  - `top_level_features_from_materialized_ssot`
  - `emit_evidence_policy_context`
- Dodano walidacje zakresow i zgodnosci clean/degraded sample counts.
- W tym samym inert surface utrzymano neutralne lower-bound pola strict-threshold:
  - `min_burst_ratio = 0.0`
  - `min_flipper_presence_ratio = 0.0`
  - `min_cpv_other_pool_activity = 0.0`
- Te pola sa config-only w PR1. Nie dodano konsumenta policy i nie zmieniono BUY/REJECT/TIMEOUT.

Zmiana 3: re-export config enums
- Plik: `ghost-brain/src/config/mod.rs`
- Utrzymano publiczny surface dla reszty systemu bez obchodzenia modulu config.

Zmiana 4: bazowy config repo
- Plik: `ghost-brain/ghost_brain_config.toml`
- Dodano bazowe fail-closed wartosci PR1 dla kompatybilnosci i jawnego kontraktu.

Zmiana 5: rollout profiles R37/R38
- Pliki:
  - `configs/rollout/ghost_brain_selector_dataset_sampler_r37_threshold_probe_maxwait3789_fsc_off.toml`
  - `configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`
- R37 ustawiono jako evidence-first/log-only.
- R38 ustawiono jako selector-only dla temporal carry-forward policy.

Zmiana 6: additive evidence shell types
- Plik: `ghost-core/src/checkpoint/types.rs`
- Dodano:
  - `MetricEvidenceQuality`
  - `TemporalMetricSource`
  - `TemporalAnchorReachedBy`
  - `CpvEvidenceContext`
  - `TemporalMetricEvidenceContext`
- Shell types pozostaja neutralne wobec scoringu i gotowe pod nastepne PR-y.

Zmiana 7: testy evidence serde/backward compatibility
- Plik: `ghost-core/tests/feature_builder_tests.rs`
- Dodano testy:
  - snake_case serialization dla evidence shell enums/structs
  - shell-context serialization dla CPV/temporal evidence metadata

## 7. Walidacja dzialan naprawczych

### Targeted validation

| Walidacja | Komenda | Wynik | Status |
|---|---|---|---|
| Rustfmt touched crates | `cargo fmt --package ghost-brain --package ghost-core` | passed | PASS |
| Diff hygiene | `git diff --check` | passed | PASS |
| Config PR1 tests | `cargo test -p ghost-brain gatekeeper_v2_ -- --nocapture` | targeted PR1 tests passed | PASS |
| Evidence serde tests | `cargo test -p ghost-core --test feature_builder_tests -- --nocapture` | targeted shell tests passed | PASS |

### Backward compatibility validated

- Stary config bez nowych pol pozostaje deserializowalny.
- Kazda wartosc nowych enumow deserializuje sie poprawnie.
- Shell typy serializuja sie w `snake_case` i pozostaja neutralne wobec runtime path.

### Uwaga o szerszym pakiecie

`cargo test -p ghost-brain --lib -- --nocapture` ujawnia istniejace, niezalezne failure w innych obszarach (`chaos::amm_math`, `oracle::ultrafast::sobp`, `pipeline::jito_processor` i pokrewne). Ten pakiet nie jest poprawnym gate dla PR1 i nie zostal uzyty jako kryterium akceptacji tej zmiany.

## 8. Ryzyka resztkowe / czego PR1 jeszcze nie robi

- PR1 nie materializuje jeszcze nowych CPV/temporal evidence fields do `MaterializedFeatureSet`.
- PR1 nie zmienia aktywnego scoringu strict metric threshold gate.
- PR1 nie promuje degraded low-sample CPV do actionable policy behavior.
- PR1 nie rozstrzyga jeszcze, ktore temporal metrics moga byc legalnie carried-forward poza log-only/selector-only policy surface.
- Kolejne PR-y musza dopiac policy wiring i runtime population bez naruszenia SSOT.

## 9. Scope out

Poza zakresem pozostaly:
- Gatekeeper policy ordering i score accumulation,
- BUY/REJECT/TIMEOUT semantics,
- DecisionLogger schema rewrite,
- dataset vectors / temporal deltas / runtime evidence population,
- live execution / sender / reconciliation,
- legacy scoring revival,
- szerokie zmiany w `SybilResistanceFeatures` lub innych istniejacych structs, ktore wymusilyby masowy churn w test fixtures bez potrzeby PR1.
