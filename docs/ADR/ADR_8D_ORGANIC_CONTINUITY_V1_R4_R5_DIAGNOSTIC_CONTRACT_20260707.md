# ADR-8D: Organic Continuity V1 R4/R5 diagnostic evidence contract

Status: IMPLEMENTED / TARGETED_VALIDATION_COMPLETED
Typ: ADR-8D / diagnostic evidence-vector contract
Data: 2026-07-07
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Zakres: PR1 Organic Continuity V1 jako diagnostic-only evidence vector; bez Gatekeeper policy, bez BUY/REJECT, bez selector score, bez TX/Jito/live path
Poziom ryzyka: LOW-MEDIUM

Dotkniete moduly/pliki:
- `ghost-core/src/checkpoint/types.rs`
- `ghost-core/src/checkpoint/mod.rs`
- `ghost-core/tests/organic_continuity_contract_tests.rs`
- `PLANS/DO_REALIZACJI/PLAN_PR1_ORGANIC_CONTINUITY_V1_R4_R5_DIAGNOSTIC_CONTRACT_20260707.md`
- `docs/ADR/ADR_8D_ORGANIC_CONTINUITY_V1_R4_R5_DIAGNOSTIC_CONTRACT_20260707.md`

Powiazane evidence:
- `reports/selector/r5_organic_continuity_availability_audit.md`
- `reports/selector/r5_edge_candidate_rules.md`
- `reports/selector/r4_oos_validation_of_r5_edge_rules.md`
- `reports/selector/l2_edge_filter_candidates_r4_r5_20260707.md`

Uwaga o szablonie:
Literalna sciezka z instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Dokument zachowuje istniejacy lokalny format ADR-8D uzywany w repo.

## 1. Przygotowanie i dzialania wstepne

Cel:
Zaktualizowac PR1 Organic Continuity V1 po R5 discovery i R4 OOS validation tak, aby nie kodowal directional policy score ani zalozenia, ze wyzszy buy pressure jest pozytywny.

Pierwotny problem:
R5 discovery i R4 OOS validation pokazaly OOS_PASS dla kohort niskiego lub ograniczonego buy pressure, m.in.:

```text
sol_buy_ratio <= 0.5099 / 0.5173 / 0.5326
organic_broadening.buy_ratio_mean <= 0.25
buy_ratio_max <= 0.6
buy_count <= 4
```

To przeczy pierwotnej intuicji, ze wysoki buy ratio powinien byc nagradzany jako Organic Continuity.

## 2. R4/R5 Evidence Update

R4/R5 offline evidence indicates low/limited buy pressure cohorts reduce median loss and left tail.
This contradicts the initial assumption that higher buy ratio should be rewarded.
Therefore Organic Continuity V1 is diagnostic-only and cannot be used for Gatekeeper policy promotion.

Konsekwencja:
- PR1 jest diagnostic-only.
- R4/R5 thresholds sa tylko bucket/reason evidence.
- Brak promotion score.
- Brak policy score.
- Brak runtime filter.
- Brak Gatekeeper policy promotion.

## 3. Claim boundaries

PR1 utrwala nastepujace granice w planie i w typowanym kontrakcie:

```text
diagnostic_only=true
shadow_only=true
changes_gatekeeper_decision=false
changes_execution=false
production_promotion_allowed=false
policy_score=false
runtime_filter=false
```

Te wartosci sa serializowane przez `OrganicContinuityClaimBoundariesV1` i testowane w `organic_continuity_contract_tests`.

## 4. Strategia naprawy

Przyjeta strategia:
- Dodac typowany evidence vector w `ghost-core`, wyprowadzany z `MaterializedFeatureSet`.
- Zachowac `MaterializedFeatureSet` jako SSOT i nie czytac mutable runtime state.
- Wystawic raw organic fields oraz context fields bez interpretacji policy.
- Dodac bucket/reason code surface dla R4/R5 diagnostic buckets.
- Dodac disabled/not-implemented experimental-score metadata z flagami:
  - `experimental_diagnostic_score = true`
  - `not_policy_score = true`
  - `not_promotion_candidate = true`
  - `direction_unvalidated = true`
- Dodac deterministic contract hash helper dla score metadata.
- Zapisac PR1 plan w `PLANS/DO_REALIZACJI`.

Granice:
- Nie zmieniono Gatekeeper BUY/REJECT.
- Nie zmieniono `selector_shadow_score_combined_simple_v1`.
- Nie zmieniono `organic_broadening_passes`.
- Nie zmieniono TX/Jito/live path.
- Nie dodano runtime promotion ani runtime filter.
- Nie uzyto lifecycle/outcome/PnL/terminal/exit jako input feature.

## 5. Przeprowadzone akcje naprawcze

Zmiana 1: diagnostic evidence vector
- Plik: `ghost-core/src/checkpoint/types.rs`
- Dodano:
  - `OrganicContinuityEvidenceV1`
  - `OrganicContinuityRawOrganicFieldsV1`
  - `OrganicContinuityContextFieldsV1`
  - `OrganicContinuityAvailabilityV1`
  - `OrganicContinuityBucketReasonV1`
  - `OrganicContinuityMissingReasonV1`
  - `OrganicContinuityClaimBoundariesV1`
  - `OrganicContinuitySourceV1`

Zmiana 2: optional experimental score metadata
- Plik: `ghost-core/src/checkpoint/types.rs`
- Dodano:
  - `OrganicContinuityExperimentalScoreV1`
  - `OrganicContinuityExperimentalScoreStatusV1`
  - `organic_continuity_experimental_score_contract_hash`
- Score ma status `not_implemented`, `value = None`, direction unvalidated i nie jest policy/promotion candidate.

Zmiana 3: MaterializedFeatureSet helper
- Plik: `ghost-core/src/checkpoint/types.rs`
- Dodano:
  - `MaterializedFeatureSet::organic_continuity_evidence_v1()`
  - `OrganicContinuityEvidenceV1::from_materialized_features(...)`
- Konstruktor czyta tylko `MaterializedFeatureSet` i sanitizuje non-finite `f64` do `Option<f64>`.

Zmiana 4: publiczny re-export
- Plik: `ghost-core/src/checkpoint/mod.rs`
- Dodano re-export nowych typow i helpera contract hash.

Zmiana 5: targeted tests
- Plik: `ghost-core/tests/organic_continuity_contract_tests.rs`
- Dodano testy wymagane przez plan PR1.

Zmiana 6: plan SSOT
- Plik: `PLANS/DO_REALIZACJI/PLAN_PR1_ORGANIC_CONTINUITY_V1_R4_R5_DIAGNOSTIC_CONTRACT_20260707.md`
- Dodano R4/R5 Evidence Update, claim boundaries, forbidden behavior, evidence vector fields, experimental score policy, tests i acceptance gates.

## 6. Walidacja dzialan naprawczych

Targeted validation:

| Walidacja | Komenda | Wynik | Status |
|---|---|---|---|
| Rustfmt ghost-core | `cargo fmt --package ghost-core` | passed | PASS |
| Organic Continuity contract tests | `cargo test -p ghost-core --test organic_continuity_contract_tests -- --nocapture` | 6 passed | PASS |
| Existing feature builder/backcompat tests | `cargo test -p ghost-core --test feature_builder_tests -- --nocapture` | 5 passed | PASS |
| Existing PR1 foundation regression check | `cargo test -p ghost-core --test pr1_contracts_foundations -- --nocapture` | 3 passed, 1 failed in unrelated `AccountStateUpdate` bincode roundtrip with `InvalidTagEncoding(104)` | FAIL_EXISTING_OR_UNRELATED |

Uwagi:
- `ghost-core` nadal emituje istniejace ostrzezenia o deprecated ShadowLedger/bootstrap paths i unused symbols. Nie sa skutkiem tej zmiany.
- Nie uruchamiano pelnego workspace test suite, poniewaz zakres PR1 jest lokalny do `ghost-core` checkpoint/evidence contract.
- Dodatkowy failure `pr1_contracts_foundations::foundational_types_serialize_and_deserialize_roundtrip` wystepuje na `AccountStateUpdate` bincode roundtrip i nie dotyka Organic Continuity V1, `MaterializedFeatureSet`, Gatekeeper policy ani selector score.

## 7. Test coverage mapping

Wymaganie: evidence vector serializes all raw organic fields.
- Test: `evidence_vector_serializes_all_raw_organic_fields_and_boundaries`

Wymaganie: low buy ratio is represented neutrally, not rejected.
- Test: `low_buy_ratio_is_diagnostic_neutral_not_rejected`

Wymaganie: high buy ratio is represented neutrally, not rewarded as policy.
- Test: `high_buy_ratio_is_diagnostic_neutral_not_policy_rewarded`

Wymaganie: claim_boundaries forbid runtime promotion.
- Test: `claim_boundaries_forbid_runtime_promotion`

Wymaganie: no outcome/lifecycle fields are used.
- Test: `organic_continuity_contract_excludes_outcome_lifecycle_fields`

Wymaganie: contract hash changes on schema/weight change if experimental score exists.
- Test: `experimental_score_contract_hash_changes_with_schema_or_weight_seed`

## 8. Ryzyka resztkowe / czego PR1 nadal nie robi

- PR1 nie loguje jeszcze `organic_continuity_evidence_v1` w decision JSONL.
- PR1 nie podlacza evidence vector do Gatekeeper policy.
- PR1 nie zmienia selector score.
- PR1 nie zmienia runtime execution.
- PR1 nie promuje R4/R5 thresholds do zadnych aktywnych filtrow.
- Bucket reasons sa diagnostic-only i nie moga byc traktowane jako acceptance/rejection reasons bez osobnego PR.

## 9. Scope out

Poza zakresem pozostaly:
- Gatekeeper V2/V2.5/V3 policy behavior.
- `organic_broadening_passes`.
- `selector_shadow_score_combined_simple_v1`.
- TX/Jito/live path.
- Shadow close / active close.
- Runtime sidecar promotion.
- Legacy HyperPrediction / old scoring.
- Lifecycle/outcome/PnL/terminal/exit features jako input.

## 10. Rollback

Rollback jest lokalny:
- usunac nowe typy i helpery Organic Continuity V1 z `ghost-core/src/checkpoint/types.rs`;
- usunac re-exporty z `ghost-core/src/checkpoint/mod.rs`;
- usunac test `ghost-core/tests/organic_continuity_contract_tests.rs`;
- usunac plan PR1 i ADR.

Poniewaz zmiana nie jest podpieta do policy/runtime, rollback nie wymaga migracji configu ani replay artifacts.
