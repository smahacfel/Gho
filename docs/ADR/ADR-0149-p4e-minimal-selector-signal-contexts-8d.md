# ADR-8D: P4E Minimal Selector Signal Contexts

Status: Accepted
Typ: Offline data materialization / selector research evidence
Data: 2026-06-09
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `main`
Commit/PR: `PR-P4E: materialize minimal selector signal contexts` (`8dd0aa5`) plus local post-review buyer-quality corrections pending commit
Zakres: selector offline training-view context materialization only
Dotknięte moduły/pliki:
- `scripts/build_selector_buyer_quality_context.py`
- `scripts/build_selector_funding_graph_context.py`
- `scripts/build_selector_training_view.py`
- `scripts/build_selector_phase3_r2only.py`
- `scripts/test_selector_pipeline.py`
Powiązane runy/logi/raporty:
- `selector-phase1-pumpfun-sol-v1-20260608-r19-feature-rich-r2diag-simcov-final`
- `selector-phase1-pumpfun-sol-v1-20260608-r21-shadow-score-flowmap-final`
- `shadow-burnin-v3-selector-dataset-r19-feature-rich-r2diag-simcov`
- `shadow-burnin-v3-selector-dataset-r21-shadow-score-flow-map-smoke`
- local, uncommitted P4E context artifacts:
  - `datasets/selector/*/buyer_quality_context_v1.jsonl`
  - `reports/selector/*/buyer_quality_context_manifest_v1.json`
  - `datasets/selector/*/funding_graph_context_v1.jsonl`
  - `reports/selector/*/funding_graph_context_manifest_v1.json`
Poziom ryzyka: Low for runtime, because the change is offline-only Python tooling and does not touch Gatekeeper, runtime, execution, or send path. Medium for research interpretation, because buyer-quality evidence quality depends on parser coverage and funding/cabal context is mostly unavailable on r19/r21. A post-review pass found and corrected buyer-quality materialization defects before marking the ADR complete.

## 1. Przygotowanie i działania wstępne

Plan początkowy:
After P4C closed simple evidence-gated candidates as NO-GO and P4D identified missing signal families, add minimal decision-time-safe data materialization for two new families: buyer/wallet quality and funding/cabal graph evidence. Do not build a new score, do not start runtime, do not tune Gatekeeper, and do not change execution.

Rzeczywisty przebieg:
Two offline context builders were added. `buyer_quality_context_v1` materializes repeat buyer and prior pool participation proxies before candidate cutoff. `funding_graph_context_v1` materializes funding-lane status, coverage, known source aggregation, clustering, and unknown/unavailable reasons. `selector_training_view_v1` and `build_selector_phase3_r2only.py` were extended with optional LEFT JOIN inputs for both contexts. The R2 denominator and leakage contracts were preserved.

Odchylenia od planu:
The r21 event set was large enough that full event-wallet-history processing was bounded by an explicit event-byte threshold. For that case, the buyer quality builder emitted `coordination_proxy_status_only` instead of silently attempting an expensive full-history scan or fabricating repeat-buyer fields. Funding graph context on r19/r21 was materialized as `missing_funding_lane`, not as clean zero-valued cabal evidence.

Post-review correction:
An implementation review found three buyer-quality issues after the initial P4E commit: the parser prefilter skipped valid `is_buy=true` events without a literal `"buy"` value, `bq_cross_pool_velocity_*` could lose per-wallet alignment, and proxy mode ignored non-default `--first-n`. These were corrected in the working tree and covered by regression tests before this ADR was updated.

## 2. Wykorzystane skills/sub-agenci

Nazwa: `large-data-analytics`
Powód użycia: The task is offline event-stream/context materialization for new selector signal families.
Zakres użycia: Define bounded buyer-history extraction, funding evidence aggregation, status coverage, and context manifests.
Wynik: Two offline builders now emit auditable JSONL contexts and manifests without runtime changes.
Ograniczenia: The builders expose available evidence and coverage; they do not prove that the new features have predictive edge.

Nazwa: `statistical-research-engine`
Powód użycia: The task follows a failed candidate validation path and must avoid leakage, missing-as-zero behavior, and R2 denominator drift.
Zakres użycia: Preserve decision-time cutoff safety, keep R2 labels out of buyer quality history, and keep missing funding evidence explicit.
Wynik: Tests cover future-activity exclusion, unknown-not-zero semantics, and training-view denominator preservation.
Ograniczenia: No new candidate scoring or cross-run lift validation is performed in P4E.

Nazwa: `ghost-execution`
Powód użycia: The repository requires strict shadow/live separation and auditability of selector evidence.
Zakres użycia: Confirm that the change remains offline-only and does not affect Gatekeeper policy, runtime decisions, execution, send path, or lifecycle behavior.
Wynik: P4E is limited to Python selector dataset tooling and does not modify Rust runtime or active decision paths.
Ograniczenia: Runtime capture quality for funding/cabal evidence remains a separate future concern.

## 3. Opis problemu - 3W2H

What:
The current selector research view lacked explicit buyer/wallet-quality history and funding/cabal graph evidence. After P4C, simple evidence-gated candidates based on existing feature families were NO-GO, so further score-grid tuning on the same features was not justified.

Where:
The gap exists in the offline selector dataset path, especially `selector_training_view_v1.jsonl` and the Phase3 R2-only selector artifacts for r19 and r21.

Why it matters:
Without explicit new signal-family contexts, the next model redesign would either keep reusing exhausted feature families or accidentally treat unavailable buyer/funding evidence as safe zeros. That would repeat the same failure mode that rejected `combined:simple_feature_score_v1`.

How observed:
P4C produced no stable simple evidence-gated candidate. P4D identified two missing families as the next bounded research direction: buyer/wallet quality and funding/cabal evidence. Existing r19/r21 training views did not contain enough materialized fields to test these families honestly.

How many / scale:
P4E was rebuilt locally on two frozen selector scopes:
- r19 candidate rows: `7195`
- r19 effective R2 training denominator: `1455`
- r21 candidate rows: `11961`
- r21 effective R2 training denominator: `2106`

Evidence:
Local P4E artifacts showed:
- r19 buyer quality rows written: `7195`
- r19 buyer quality status counts: `clean=6243`, `no_prior_history_observed=199`, `unknown_no_buyer_evidence=753`
- r21 buyer quality rows written: `11961`
- r21 buyer quality status counts: `proxy_status_only=11959`, `unknown=2`
- r19 funding graph rows written: `7195`, all `missing_funding_lane`
- r21 funding graph rows written: `11961`, all `missing_funding_lane`

## 4. Przyczyna źródłowa

Root cause:
The selector training view had reached the limit of its current feature families. It could express flow/GK/evidence-sufficiency fields, but it did not materialize buyer-history context or funding-source/cabal context as first-class, cutoff-safe fields.

Mechanizm błędu:
The previous candidate family could rank candidates using available flow/GK metrics, but it could not distinguish whether early buyers were repeat/experienced participants, whether a buyer cluster shared funding sources, or whether funding evidence was unavailable. This forced model redesign to operate without the signal families P4D identified as necessary.

Miejsce:
Offline selector dataset tooling:
- training view construction
- Phase3 R2-only context joins
- missing buyer/funding context builders

Skutek:
Further score tuning would be feature fishing on exhausted inputs. Funding unknowns could be misread as benign if not explicitly materialized. Buyer-quality hypotheses could not be tested without rebuilding context fields.

Dowód:
P4C returned `P4C_NEEDS_NEW_FEATURE_FAMILY` with no stable candidate IDs. P4E rebuilds show funding lane is unavailable on r19/r21 and buyer quality has asymmetric coverage between r19 full event history and r21 proxy/status-only mode.

Post-review implementation defects:
- The initial buyer-quality parser used an over-aggressive line prefilter requiring a literal `"buy"` string before JSON parsing. This contradicted the domain function that accepts `is_buy is True`.
- Cross-pool velocity initially paired `prior_pool_counts` and `first_seen_values` with `zip`, losing per-wallet alignment when some early buyers had no prior history.
- Proxy mode used `DEFAULT_FIRST_N` instead of the CLI-provided `--first-n`.

Odrzucone hipotezy:
- Runtime score plumbing failure: rejected by prior P3L/P3M work; runtime score coverage/parity had passed.
- Gatekeeper threshold issue: rejected as out of scope and unsupported by R21 candidate validation.
- Another simple score grid on the same features: rejected by P4C NO-GO.
- Treat funding missing as clean/no-risk: rejected; P4E materializes `missing_funding_lane`.

## 5. Strategia naprawy

Przyjęta strategia:
Add minimal offline context materialization before any new model work. The context is joined into training view only when explicitly provided. Missing or unavailable evidence is statused, not converted to zero or safe values.

Zakres ingerencji:
- Add `buyer_quality_context_v1` builder.
- Add `funding_graph_context_v1` builder.
- Add optional LEFT JOIN support in `build_selector_training_view.py`.
- Add pass-through support in `build_selector_phase3_r2only.py`.
- Add unit tests for cutoff safety, missing semantics, funding-lane availability, and denominator preservation.

Czego nie zmieniano:
- Rust runtime
- Gatekeeper policy
- BUY / REJECT / TIMEOUT behavior
- execution or send path
- selector score or thresholds
- R2 label semantics
- candidate universe semantics
- active rollout configs

Ryzyka:
- r21 buyer-quality context is currently proxy/status-only under the configured event-byte bound, so it is not equivalent to full event-wallet-history context.
- funding graph context shows `missing_funding_lane` on r19/r21, so funding/cabal scoring must not be attempted without a capture-lane fix or a scope with actual funding evidence.
- Generated P4E artifacts are local evidence and are intentionally not committed.
- The initial P4E commit had buyer-quality evidence-quality defects; the corrected version must be used for any follow-up rebuild or candidate probe.

Odrzucone alternatywy:
- Implement a new score immediately: rejected because P4E is data materialization only.
- Add runtime buyer/funding capture changes: rejected because P4E is offline-only and runtime is explicitly out of scope.
- Compute wallet prior R2 success/failure counts: rejected for P4E because historical R2-derived wallet success requires a separate cutoff-safe proof.
- Load massive r21 event history without bounds: rejected to avoid unbounded processing; explicit proxy/status-only mode is safer and auditable.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/moduł: `scripts/build_selector_buyer_quality_context.py`
- Co zmieniono: Added an offline builder for `buyer_quality_context_v1.jsonl` and `buyer_quality_context_manifest_v1.json`.
- Dlaczego: To materialize decision-time-safe buyer quality proxies before any new candidate scoring.
- Efekt: r19 can be built with full event wallet history; r21 falls back to explicit `coordination_proxy_status_only` under the event-byte bound instead of fabricating history.

Zmiana 2:
- Plik/moduł: `scripts/build_selector_funding_graph_context.py`
- Co zmieniono: Added an offline builder for `funding_graph_context_v1.jsonl` and `funding_graph_context_manifest_v1.json`.
- Dlaczego: To expose funding/cabal evidence status and known-source aggregation without treating unavailable funding as safe.
- Efekt: r19/r21 funding context is materialized as `missing_funding_lane`, preserving the blocker for future funding capture work.

Zmiana 3:
- Plik/moduł: `scripts/build_selector_training_view.py`
- Co zmieniono: Added optional `--buyer-quality-context` and `--funding-graph-context` joins through generic prefixed context loading/attachment.
- Dlaczego: To add `bq_*` and `fg_*` fields to `selector_training_view_v1` only when context files are explicitly provided.
- Efekt: The training view gains new signal-family fields and manifest coverage summaries while preserving R2 denominator semantics.

Zmiana 4:
- Plik/moduł: `scripts/build_selector_phase3_r2only.py`
- Co zmieniono: Added pass-through CLI arguments and manifest/provenance reporting for buyer quality and funding graph contexts.
- Dlaczego: Phase3 R2-only rebuilds need a stable path to include P4E contexts without changing old behavior when omitted.
- Efekt: r19/r21 Phase3 can be rebuilt with P4E contexts and still report `PASS_R2_ONLY_DRAFT` with leakage PASS.

Zmiana 5:
- Plik/moduł: `scripts/test_selector_pipeline.py`
- Co zmieniono: Added P4E tests for buyer quality cutoff safety, future-activity exclusion, no-history unknown status, funding source materialization, unavailable funding lane, training-view joins, and R2 denominator preservation.
- Dlaczego: The new contexts are only useful if they avoid leakage and missing-as-zero behavior.
- Efekt: The full selector pipeline Python test suite passes with the new context builders.

Zmiana 6:
- Plik/moduł: `scripts/build_selector_buyer_quality_context.py`
- Co zmieniono: Removed the literal `"buy"` line prefilter, corrected velocity computation to preserve per-wallet history alignment, and passed the CLI `--first-n` value into proxy context rows.
- Dlaczego: The initial implementation could silently drop valid `is_buy=true` PoolTransaction rows, understate `bq_cross_pool_velocity_*`, and ignore a configured first-N value in proxy mode.
- Efekt: Valid `is_buy=true` events are parsed, velocity is computed from the matching wallet's first-seen history, and proxy mode respects `--first-n`.

Zmiana 7:
- Plik/moduł: `scripts/test_selector_pipeline.py`
- Co zmieniono: Added regression tests for `is_buy=true` without a literal `"buy"` value, per-wallet velocity alignment, and proxy-mode `--first-n`.
- Dlaczego: These were the concrete post-review counterexamples that could corrupt offline buyer-quality evidence.
- Efekt: The selector pipeline suite now covers the buyer-quality parser and velocity edge cases directly.

## 7. Walidacja działań naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowód |
|---|---|---|---|---|
| Build | `python3 -m py_compile scripts/build_selector_buyer_quality_context.py scripts/build_selector_funding_graph_context.py scripts/build_selector_training_view.py scripts/build_selector_phase3_r2only.py scripts/test_selector_pipeline.py` | No syntax errors | PASS | Local command output |
| Unit | `python3 -m unittest scripts.test_selector_pipeline -v` | `Ran 143 tests`, `OK`, `skipped=2` | PASS | Local command output |
| Diff hygiene | `git diff --check -- scripts/build_selector_buyer_quality_context.py scripts/build_selector_funding_graph_context.py scripts/build_selector_training_view.py scripts/build_selector_phase3_r2only.py scripts/test_selector_pipeline.py` | No whitespace/errors | PASS | Local command output |
| Commit scope check | `git show --stat --oneline 8dd0aa5` | Commit contains only 5 P4E tooling files | PASS | Local command output |
| Post-review local diff scope | `git diff --stat -- scripts/build_selector_buyer_quality_context.py scripts/test_selector_pipeline.py docs/ADR/ADR-0149-p4e-minimal-selector-signal-contexts-8d.md` | Local post-review changes are limited to buyer-quality builder, tests, and this ADR | PASS | Local command output |
| r19 buyer quality rebuild | `python3 scripts/build_selector_buyer_quality_context.py --root /root/Gho --scope selector-phase1-pumpfun-sol-v1-20260608-r19-feature-rich-r2diag-simcov-final --runtime-scope shadow-burnin-v3-selector-dataset-r19-feature-rich-r2diag-simcov --json` | `rows_written=7195`, status counts `clean=6243`, `no_prior_history_observed=199`, `unknown_no_buyer_evidence=753` | PASS | Local uncommitted manifest |
| r21 buyer quality rebuild | `python3 scripts/build_selector_buyer_quality_context.py --root /root/Gho --scope selector-phase1-pumpfun-sol-v1-20260608-r21-shadow-score-flowmap-final --runtime-scope shadow-burnin-v3-selector-dataset-r21-shadow-score-flow-map-smoke --json` | `rows_written=11961`, `history_source=coordination_proxy_status_only`, status counts `proxy_status_only=11959`, `unknown=2` | PASS with limitation | Local uncommitted manifest |
| r19 funding graph rebuild | `python3 scripts/build_selector_funding_graph_context.py --root /root/Gho --scope selector-phase1-pumpfun-sol-v1-20260608-r19-feature-rich-r2diag-simcov-final --runtime-scope shadow-burnin-v3-selector-dataset-r19-feature-rich-r2diag-simcov --json` | `rows_written=7195`, all `missing_funding_lane` | PASS as status materialization | Local uncommitted manifest |
| r21 funding graph rebuild | `python3 scripts/build_selector_funding_graph_context.py --root /root/Gho --scope selector-phase1-pumpfun-sol-v1-20260608-r21-shadow-score-flowmap-final --runtime-scope shadow-burnin-v3-selector-dataset-r21-shadow-score-flow-map-smoke --json` | `rows_written=11961`, all `missing_funding_lane` | PASS as status materialization | Local uncommitted manifest |
| r19 Phase3 rebuild with contexts | `python3 scripts/build_selector_phase3_r2only.py --scope selector-phase1-pumpfun-sol-v1-20260608-r19-feature-rich-r2diag-simcov-final --root /root/Gho --gatekeeper-feature-context datasets/selector/selector-phase1-pumpfun-sol-v1-20260608-r19-feature-rich-r2diag-simcov-final/gatekeeper_feature_context_v1.jsonl --buyer-quality-context datasets/selector/selector-phase1-pumpfun-sol-v1-20260608-r19-feature-rich-r2diag-simcov-final/buyer_quality_context_v1.jsonl --funding-graph-context datasets/selector/selector-phase1-pumpfun-sol-v1-20260608-r19-feature-rich-r2diag-simcov-final/funding_graph_context_v1.jsonl --json` | `PASS_R2_ONLY_DRAFT`, `effective_r2_training_denominator_rows=1455`, leakage PASS | PASS | Local uncommitted Phase3 manifest |
| r21 Phase3 rebuild with contexts | `python3 scripts/build_selector_phase3_r2only.py --scope selector-phase1-pumpfun-sol-v1-20260608-r21-shadow-score-flowmap-final --root /root/Gho --gatekeeper-feature-context datasets/selector/selector-phase1-pumpfun-sol-v1-20260608-r21-shadow-score-flowmap-final/gatekeeper_feature_context_v1.jsonl --buyer-quality-context datasets/selector/selector-phase1-pumpfun-sol-v1-20260608-r21-shadow-score-flowmap-final/buyer_quality_context_v1.jsonl --funding-graph-context datasets/selector/selector-phase1-pumpfun-sol-v1-20260608-r21-shadow-score-flowmap-final/funding_graph_context_v1.jsonl --json` | `PASS_R2_ONLY_DRAFT`, `effective_r2_training_denominator_rows=2106`, leakage PASS | PASS | Local uncommitted Phase3 manifest |
| Replay/simulation | Not run | P4E is offline context materialization and does not change runtime/simulation | N/A | Scope boundary |
| Guard negative case | Unit tests for future activity, no-history unknown, unavailable funding lane, and denominator preservation | Future post-cutoff buyer activity does not increase prior participation; funding unavailable remains unavailable; R2 denominator preserved | PASS | Python unit tests |
| Buyer parser regression | `python3 -m unittest scripts.test_selector_pipeline.SelectorPipelineTests.test_buyer_quality_context_accepts_is_buy_without_literal_buy_value -v` | Valid `is_buy=true` PoolTransaction row is counted as buyer evidence | PASS | Python unit test |
| Buyer velocity regression | `python3 -m unittest scripts.test_selector_pipeline.SelectorPipelineTests.test_buyer_quality_context_velocity_keeps_wallet_alignment -v` | Repeat buyer with two prior pools yields non-zero velocity instead of being paired with a fresh buyer's zero count | PASS | Python unit test |
| Proxy first-N regression | `python3 -m unittest scripts.test_selector_pipeline.SelectorPipelineTests.test_buyer_quality_context_proxy_mode_respects_first_n -v` | Proxy context uses CLI `--first-n` instead of `DEFAULT_FIRST_N` | PASS | Python unit test |

Wniosek walidacyjny:
P4E is validated as offline data materialization after post-review buyer-quality fixes. The tooling compiles, the selector test suite passes, targeted regression tests cover the reviewed counterexamples, r19/r21 frozen scopes rebuild with the new contexts, and Phase3 keeps leakage PASS plus stable R2 denominators.

Ograniczenia walidacji:
This validation does not prove predictive edge for buyer quality or funding graph features. r21 buyer quality is proxy/status-only under the event-byte bound, and r19/r21 funding graph context is currently `missing_funding_lane`. No runtime, Gatekeeper, execution, or send-path behavior was tested because none was changed.

## 8. Wdrożone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: Unit tests
- Co zabezpiecza: Buyer quality history must be cutoff-safe and must not use future pool activity.
- Kiedy się aktywuje: During `scripts.test_selector_pipeline`.
- Jak przetestowano: `test_buyer_quality_context_counts_repeat_buyers_before_cutoff` and `test_buyer_quality_context_does_not_use_future_pool_activity`.
- Co pozostaje poza zakresem: It does not validate predictive strength of repeat-buyer fields.

Guardrail 2:
- Typ: Missing-policy tests
- Co zabezpiecza: No-history and unavailable funding evidence must remain unknown/unavailable, not safe zero.
- Kiedy się aktywuje: During `scripts.test_selector_pipeline`.
- Jak przetestowano: `test_buyer_quality_context_marks_no_history_as_unknown_not_zero` and `test_funding_graph_context_marks_unavailable_funding_lane`.
- Co pozostaje poza zakresem: It does not repair the missing funding lane.

Guardrail 3:
- Typ: Training-view join and denominator tests
- Co zabezpiecza: P4E contexts are additive LEFT JOINs and do not alter R2 denominator.
- Kiedy się aktywuje: During `scripts.test_selector_pipeline`.
- Jak przetestowano: `test_training_view_joins_buyer_quality_and_funding_context` and `test_training_view_preserves_r2_denominator_with_new_context`.
- Co pozostaje poza zakresem: It does not choose a candidate score or threshold.

Guardrail 4:
- Typ: Buyer-quality parser regression tests
- Co zabezpiecza: Valid PoolTransaction BUY rows are not dropped merely because they express buy semantics as `is_buy=true` instead of a literal `"buy"` value.
- Kiedy się aktywuje: During `scripts.test_selector_pipeline`.
- Jak przetestowano: `test_buyer_quality_context_accepts_is_buy_without_literal_buy_value`.
- Co pozostaje poza zakresem: It does not validate every historical event schema variant.

Guardrail 5:
- Typ: Buyer-quality metric alignment tests
- Co zabezpiecza: `bq_cross_pool_velocity_mean/max` must preserve wallet-to-history alignment.
- Kiedy się aktywuje: During `scripts.test_selector_pipeline`.
- Jak przetestowano: `test_buyer_quality_context_velocity_keeps_wallet_alignment`.
- Co pozostaje poza zakresem: It does not decide whether velocity has predictive edge.

Guardrail 6:
- Typ: Proxy-mode CLI contract test
- Co zabezpiecza: Proxy buyer-quality mode respects `--first-n`.
- Kiedy się aktywuje: During `scripts.test_selector_pipeline`.
- Jak przetestowano: `test_buyer_quality_context_proxy_mode_respects_first_n`.
- Co pozostaje poza zakresem: It does not make proxy context equivalent to full wallet-history context.

## Otwarte ryzyka / follow-up

- Funding/cabal cannot be scored on r19/r21 because both scopes materialize as `missing_funding_lane`; next funding step requires capture-lane repair or a new scope with funding evidence.
- r21 buyer quality currently uses `coordination_proxy_status_only` under the bounded event-byte threshold; a full r21 buyer-history pass would need either a more efficient indexed event source or an explicit higher processing budget.
- P4E does not build a new model. The next modeling step should only run after reviewing context coverage:
  - buyer quality probe if buyer context coverage is sufficient,
  - funding capture repair if funding lane remains unavailable,
  - no runtime or Gatekeeper tuning until a stable offline candidate is proven.
- Generated datasets/reports remain local and should not be committed unless explicitly approved as frozen evidence artifacts.
