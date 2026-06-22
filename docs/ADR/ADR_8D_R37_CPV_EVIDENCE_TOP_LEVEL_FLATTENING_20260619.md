# ADR-8D: R37 CPV evidence top-level flattening repair

Status: IMPLEMENTED / TARGETED_TEST_VERIFIED
Typ: ADR-8D / decision logger projection repair
Data: 2026-06-19
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: residual runtime gap after PR5/PR6; CPV evidence value was present in embedded SSOT but absent from top-level convenience fields
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-launcher/src/components/gatekeeper.rs`
- `docs/ADR/ADR_8D_R37_CPV_EVIDENCE_TOP_LEVEL_FLATTENING_20260619.md`

Powiazane plany i ADR:
- `PLANS/DO_REALIZACJI/PLAN_EVIDENCE_COVERAGE_CONTRACT_CPV_TEMPORAL_BURST_20260618.md`
- `docs/ADR/ADR_8D_PR2_CPV_SUCCESSFUL_BUY_COVERAGE_CONTRACT_20260619.md`
- `docs/ADR/ADR_8D_PR5_EVIDENCE_POLICY_WIRING_AND_REASON_CODES_20260619.md`
- `docs/ADR/ADR_8D_PR6_SELECTOR_EVIDENCE_CONTEXT_EXPORT_20260619.md`
- `docs/ADR/ADR_8D_R37_CPV_DEGRADED_TOP_LEVEL_EMISSION_20260619.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w istniejacych ADR-ach PR1-PR6.

## 1. Przygotowanie i dzialania wstepne

Kontekst:
- Aktywny R37 runtime pozostal uruchomiony i nie byl restartowany w trakcie tej poprawki.
- User doprecyzowal, ze oceniamy tylko segment JSONL po ostatniej kompilacji, nie caly plik historyczny.
- Runtime artefakt pokazal residual gap: embedded `v3_materialized_feature_snapshot.sybil_resistance.cpv_evidence` mial `quality=degraded_low_sample` oraz wartosci CPV, ale top-level `signer_cross_pool_velocity` i `cpv_other_pool_activity` byly puste dla czesci rekordow.

Najwazniejsze ograniczenie:
Ta naprawa nie moze zmieniac denominatora CPV, strict policy, verdictow ani decyzji Gatekeepera. CPV nadal znaczy successful-buy signer cross-pool velocity, a low-sample value nadal musi byc rozpoznawalne jako degraded evidence.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: SSOT `MaterializedFeatureSet`, DecisionLogger/replay boundary, shadow/live separation.
- `large-data-analytics`: interpretacja runtime JSONL jako artefaktu danych i rozroznienie embedded evidence vs top-level convenience.
- `statistical-research-engine`: ochrona znaczenia `null`, `0`, `degraded_low_sample` i clean evidence dla selector/dataset.

Zaladowane dokumenty specjalistyczne:
- `docs/agents/decision-logging-replay-analyst.md`

Nie zmieniano:
- `gatekeeper_policy.rs`
- `cross_pool_velocity.rs`
- runtime denominator CPV
- strict metric policy
- verdict/reason taxonomy
- shadow/live execution boundary

## 3. Opis problemu - 3W2H

What:
Top-level JSONL projection nie zawsze emitowala CPV values, mimo ze embedded SSOT mial jawne CPV evidence values.

Where:
- `GatekeeperAssessment::to_buy_log()`
- pola top-level `signer_cross_pool_velocity` i `cpv_other_pool_activity`
- embedded source: `MaterializedFeatureSet.sybil_resistance.cpv_evidence`

Why it matters:
Selector/offline consumer czytajacy tylko top-level widzial falszywy brak metryki, mimo ze durable embedded snapshot posiadal wartosc i status `degraded_low_sample`. To lamie zasade evidence contract: liczba, null i degraded evidence musza byc rozroznialne, a nie ukryte przez shape JSONL.

How observed:
Artefakty R37 schema28 pokazaly przypadki:
- `cpv_evidence.quality = degraded_low_sample`
- `cpv_evidence.signer_cross_pool_velocity = Some(...)`
- `cpv_evidence.cpv_other_pool_activity = Some(...)`
- top-level `signer_cross_pool_velocity = null` / missing
- top-level `cpv_other_pool_activity = null` / missing

How fixed:
`to_buy_log()` dostal bounded fallback z `cpv_evidence` do top-level convenience fields, aktywny tylko gdy `top_level_features_from_materialized_ssot=true` i tylko dla `quality=clean` albo `quality=degraded_low_sample`.

## 4. Przyczyna zrodlowa

Root cause:
`to_buy_log()` mapowal top-level CPV fields wylacznie z policy-facing:
- `sybil.signer_cross_pool_velocity`
- `sybil.cpv_other_pool_activity`

Te pola moga byc `None`, gdy runtime/policy nie ma traktowac low-sample CPV jako clean policy evidence. Jednoczesnie embedded `cpv_evidence` moze prawidlowo zawierac wartosc wraz ze statusem `degraded_low_sample`, sample count i wymaganym progiem.

Skutek:
- policy semantics byly ostrozne,
- embedded evidence bylo poprawne,
- top-level convenience projection byla niekompletna.

## 5. Strategia naprawy

Przyjeto minimalny wariant logger-only:
- Zachowac `sybil.signer_cross_pool_velocity` jako pierwsze zrodlo, jesli juz istnieje.
- Gdy top-level SSOT flattening jest wlaczony, a evidence quality to `clean` albo `degraded_low_sample`, uzyc `sybil.cpv_evidence.*` jako fallback.
- Nie emitowac fallbacku dla `insufficient_sample`, `stale`, `unavailable`, `unavailable_source`, `not_configured`, `not_allowed`.
- Nie zmieniac policy path ani strict-gate consumption.

Konsekwencja:
Top-level JSONL bedzie wygodniejszy dla audytu i datasetu, ale prawdziwa jakosc evidence nadal musi byc czytana z embedded `cpv_evidence` oraz PR6 selector evidence columns.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: bounded CPV evidence fallback w `to_buy_log()`
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- Dodano `cpv_evidence_can_flatten`.
- Dodano `top_level_signer_cross_pool_velocity`.
- Dodano `top_level_cpv_other_pool_activity`.
- Pola top-level `GatekeeperBuyLog` teraz korzystaja z tych wartosci.

Zmiana 2: test kontraktu loggera
- Plik: `ghost-launcher/src/components/gatekeeper.rs`
- Test: `degraded_cpv_evidence_flattens_to_buy_log_without_policy_field`
- Scenariusz testu:
  - policy-facing CPV fields sa `None`;
  - embedded `cpv_evidence` ma `quality=degraded_low_sample` i wartosci;
  - przy `top_level_features_from_materialized_ssot=true` top-level pola sa emitowane;
  - przy `top_level_features_from_materialized_ssot=false` stare zachowanie zostaje zachowane.

## 7. Walidacja

Wykonane:

```bash
cargo fmt --package ghost-launcher
cargo test -q -p ghost-launcher degraded_cpv_evidence_flattens_to_buy_log_without_policy_field --lib
```

Wynik:
- targeted test: `1 passed`
- kompilacja testowa zakonczona sukcesem
- output zawieral istniejace w repo ostrzezenia `unused`, `deprecated` i podobne; nie dotycza tej zmiany

Do potwierdzenia runtime:
- Wymagany jest rebuild/restart runtime, bo aktywny R37 proces nadal pracuje na binary sprzed tej poprawki.
- Po restarcie acceptance check dla segmentu po nowej kompilacji:
  - `cpv_evidence.signer_cross_pool_velocity present` powinno implikowac top-level `signer_cross_pool_velocity present` dla `quality in {clean, degraded_low_sample}` i `top_level_features_from_materialized_ssot=true`.
  - Analogicznie dla `cpv_other_pool_activity`.
  - `insufficient_sample/unavailable/stale/not_allowed` nie powinny byc wypelniane fallbackiem.

## 8. Ryzyka i ochrona przed regresja

Ryzyko 1: model pomyli degraded low-sample z clean value.
Mitigacja:
- nie zmieniono `cpv_evidence.quality`;
- nie zmieniono PR6 evidence columns;
- top-level value jest convenience, a status pozostaje w embedded/context.

Ryzyko 2: strict policy zacznie uzywac degraded CPV tak jak clean CPV.
Mitigacja:
- nie zmieniano `gatekeeper_policy.rs`;
- nie zmieniano `sybil.signer_cross_pool_velocity` w materialized snapshot;
- fallback dziala dopiero w `to_buy_log()`, czyli w serializacji buy logu.

Ryzyko 3: stale/unavailable CPV zostanie wypelnione jako liczba.
Mitigacja:
- fallback jest ograniczony do `MetricEvidenceQuality::Clean` i `MetricEvidenceQuality::DegradedLowSample`.

Ryzyko 4: stare profile bez SSOT top-level projection zmienia zachowanie.
Mitigacja:
- fallback jest gated przez `top_level_features_from_materialized_ssot`.
- test sprawdza zachowanie `false`.

## 9. Status koncowy

Kodowo:
- residual CPV flattening gap zostal naprawiony w `to_buy_log()`.
- test kontraktowy przeszedl.

Runtime:
- aktywny run nie zostal restartowany;
- pelny dowod runtime wymaga nowej kompilacji i nowego segmentu JSONL po restarcie procesu.
