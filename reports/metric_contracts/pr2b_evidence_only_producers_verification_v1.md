# PR2B evidence-only producers — raport weryfikacyjny V1

Status: `PASS / READY_FOR_REVIEW`

Data: 2026-07-12

## 1. Git identity

- repo: `smahacfel/Gho`;
- branch: `agent/metric-contract-pr2b-evidence-producers`;
- base/start head: `55a9c5ce306e3a0c4fceb85015f5b796567073c3`;
- fetched `origin/main`: `55a9c5ce306e3a0c4fceb85015f5b796567073c3`;
- merge-base: `55a9c5ce306e3a0c4fceb85015f5b796567073c3`;
- publication head/commit jest raportowany w PR i końcowym handoffie, ponieważ
  commit nie może zawierać własnego SHA bez samoodniesienia;
- rekomendowany commit: `metric-contracts: add PR2B evidence-only producers`.

### 1.1. Zamknięta allowlista plików

1. `ghost-core/src/account_state_core/reducer.rs`
2. `ghost-core/src/account_state_core/types.rs`
3. `ghost-core/src/checkpoint/feature_builder.rs`
4. `ghost-core/src/checkpoint/types.rs`
5. `ghost-core/src/metric_contracts/evidence.rs`
6. `ghost-core/src/metric_contracts/projection.rs`
7. `ghost-core/src/metric_contracts/status.rs`
8. `ghost-core/tests/metric_contracts_v1_1_foundation.rs`
9. `ghost-core/tests/metric_contracts_v1_1_projection.rs`
10. `ghost-launcher/src/main.rs`
11. `ghost-launcher/src/metric_contracts/mod.rs`
12. `ghost-launcher/src/metric_contracts/pr2b.rs`
13. `ghost-launcher/src/oracle_runtime.rs`
14. `ghost-launcher/src/session/manager.rs`
15. `ghost-launcher/src/session/observation.rs`
16. `ghost-launcher/src/tx_intelligence/engine.rs`
17. `ghost-launcher/src/tx_intelligence/flip_v2.rs`
18. `ghost-launcher/src/tx_intelligence/mod.rs`
19. `ghost-launcher/src/tx_intelligence/sybil_metrics.rs`
20. `ghost-launcher/tests/metric_contracts_pr2a_producers.rs`
21. `ghost-launcher/tests/metric_contracts_pr2a_static_guards.rs`
22. `ghost-launcher/tests/metric_contracts_pr2b_producers.rs`
23. `ghost-launcher/tests/metric_contracts_pr2b_static_guards.rs`
24. `docs/ADR/ADR_8D_PR2B_METRIC_CONTRACT_EVIDENCE_ONLY_PRODUCERS_20260712.md`
25. `reports/metric_contracts/pr2b_evidence_only_producers_verification_v1.md`

## 2. Owner map

| Rodzina | Canonical producer | Frozen output | Compact output |
| --- | --- | --- | --- |
| FTDI | `compute_sybil_resistance_with_ftdi` | PR2A `FtdiComputation` | `FtdiDecisionProjectionV1` |
| dev-buy | `TxIntelligenceEngine` + compatibility Gatekeeper snapshot | PR2A typed snapshots | `DevBuyDecisionProjectionV1` |
| timing | TxIntelligence + RCE recent owner | PR2A typed snapshots | `TxTimingDecisionProjectionV1` |
| top3 | TxIntelligence feature selector | PR2A typed snapshot | `Top3DecisionProjectionV1` |
| funding/FSC | `FundingSourceIndex` | fingerprint-bound `FscComputation` | `FundingDecisionProjectionV1` |
| FSC status | materialization adapter z tego samego FSC computation | PR2A status evidence | `FscStatusDecisionProjectionV1` |
| Flip V2 | `TxIntelligenceEngine::flip_v2` | bounded owner-state snapshot | `FlipDecisionProjectionV1` |
| manipulation | frozen V3 materialization owner | 7 fields + 6 derived flags | `ManipulationDecisionProjectionV1` |
| reserve velocity | `AccountStateReducer` | reserves/interval/count/status | `ReserveVelocityDecisionProjectionV1` |
| recent buy/sell | RCE recent-window owner | successful-only counts | `RecentBuySellDecisionProjectionV1` |

## 3. Effective-config boundary table

`FrozenProducerBoundaryValidated`:

| Klucz | Uzasadnienie |
| --- | --- |
| `FlipCandidateDustThresholdSol` | event eligibility, brak raw event detail w compact |
| `FlipCandidateDedupeKey` | stable identity policy, full producer state only |
| `FlipCandidateDedupeCapacity` | bounded producer state only |
| `FlipCandidateEvictionPolicy` | eviction/degradation semantics ownera |
| `FlipCandidateMaxWallets` | bounded owner state only |
| `FlipCandidateReconnectBehavior` | stream-gap state ownera |

`CompactValidated` obejmuje wszystkie pozostałe keys kontraktów `FlipRatio`,
`ManipulationContradiction`, `ReserveVelocity` i `RecentBuySell`. Static guard:

- wyprowadza expected set z `METRIC_EFFECTIVE_CONFIG_KEYS_V1`;
- wymaga exact equality setów;
- wymaga `len(set) == len(table)`;
- odrzuca brak, nadmiar, duplikat i nieprzypisaną granicę.

## 4. One producer / one snapshot / two representations

Terminal materialization zamraża każdy producer output raz. Call-count guard
sprawdza dokładnie jedno wystąpienie każdego canonical callsite oraz complete
buildera wewnątrz `try_materialize_features()`.

`build_pr2b_complete_metric_contract_snapshot_v1` tworzy najpierw pełny
`MetricContractsEvidenceSetV1`, wykonuje semantic/profile validation, a następnie
jedną pure konwersję do `MetricContractDecisionEvidenceProjectionV1` z tym samym
profile/effective-config/cutoff. Test integracyjny ponownie wykonuje pure
konwersję z zachowanego full evidence i wymaga exact equality oraz identycznego
validated hash.

## 5. Exact compact proof

Closed serde field-set test wymaga dokładnych pól czterech rodzin PR2B.
Projection JSON nie może zawierać:

```text
owners
owner_id
anchor_event_identity
qualifying_sell_event_identity
legacy_fields
derived_high_flags
```

MFS source guard wymaga dokładnie jednego pola
`metric_contract_decision_projection_v1` typu
`Option<MetricContractDecisionEvidenceProjectionV1>` i zakazuje
`MetricContractsEvidenceSetV1` w `MaterializedFeatureSet`.

## 6. Atomic materialization proof

- historical JSON po usunięciu pola deserializuje `None`;
- bieżące `try_materialize_features()` zwraca pełne `Some`;
- projection przechodzi `validate_context()` i `validated_canonical_hash()`;
- pełny evidence set pozostaje lokalny i jest dropowany po pure conversion;
- błąd zwraca `MetricContractMaterializationErrorV1`, bez policy reason/verdict;
- nie istnieje zapis partial `Some`.

## 7. Resource gate

Metryka rozmiaru używa deterministycznego bincode tego samego typed compact
root. Canonical hash pozostaje RFC8785/canonical-JSON SHA-256.

| Kryterium | Wynik |
| --- | --- |
| deterministic serialized size | `2780 B` standard fixture |
| duży dozwolony fixture | `8932 B` |
| p95 target `<= 12 KiB` | PASS |
| hard max `<= 16 KiB` | PASS — guard aktywny |
| oversized payload | `ProjectionTooLarge` |
| release p50/p95/p99 | `509 / 753 / 874 us` |
| build failures | 0 w pozytywnych fixtures |
| full/projection parity failures | 0 |

Surowe benchmark logs nie są commitowane.

## 8. Pełna macierz weryfikacji

| Kontrola | Wynik |
| --- | --- |
| `cargo test -p ghost-core --test metric_contracts_v1_1_foundation` | PASS — 19/19 |
| `cargo test -p ghost-core --test metric_contracts_v1_1_projection` | PASS — 23/23 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2a_producers` | PASS — 26/26 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2a_static_guards` | PASS — 8/8 |
| `metric_contracts_pr2b_producers` | PASS — 10/10 |
| `metric_contracts_pr2b_static_guards` | PASS — 5/5 |
| Flip V2 unit state machine | PASS — 7/7 |
| AccountState reserve velocity owner | PASS — 2/2 |
| `gatekeeper_policy_tests` | PASS — 46/46 |
| `gatekeeper_v25_regression` | PASS — 42/42 |
| `refactor_invariants_tests` | PASS — 12/12 |
| `gatekeeper_v3_tests` | PASS — 9/9 |
| `session_lifecycle_tests` | PASS — 26/26 |
| reversed recent window | PASS — 1/1 |
| same-funder FSC | PASS — 1/1 |
| insufficient-known-sources FSC | PASS — 1/1 |
| DecisionLogger buy-log write | PASS — 1/1 |
| `ghost-brain replay_payload` | PASS — 5/5 |
| `cargo check -p ghost-core` | PASS |
| `cargo check -p ghost-launcher` | PASS |
| targeted Clippy, core | PASS — exit 0 |
| targeted Clippy, launcher + PR2A/PR2B tests | PASS — exit 0 |
| `cargo fmt --all -- --check` | PASS |
| `git diff --check` | PASS |

Targeted Clippy raportuje istniejący szeroki baseline warningów poza
zmienionymi liniami. Nowy `double_must_use` i nieużywany test wrapper wykryte w
trakcie walidacji zostały usunięte.

### 8.1. Oddzielony baseline failure

`cargo test -p ghost-brain --lib selector_shadow_score` nadal daje `8 passed,
1 failed`: `test_selector_shadow_score_filters_non_finite_feature_values`.
`ghost-brain/src/oracle/decision_logger.rs` jest bitowo identyczny z base
(SHA-256 `22bf380765fb9dfa5ae38e9f7340b9d9c5ba3a467d0b2eca783344787bf64425`)
i ma pusty diff. PR2B nie zmienia selector score.

## 9. Forbidden scope proof

Wszystkie poniższe pliki mają identyczny SHA-256 jak base i
`git diff --exit-code <base> -- <path>` zwraca `0`:

| SHA-256 | Zamrożona powierzchnia |
| --- | --- |
| `5ec6a766e8bb2d9b0cacdc39d7146b5fac056e0cf57042b75b4077a82fd9210c` | `ghost-launcher/src/components/gatekeeper_policy.rs` |
| `22bf380765fb9dfa5ae38e9f7340b9d9c5ba3a467d0b2eca783344787bf64425` | `ghost-brain/src/oracle/decision_logger.rs` |
| `feca4ba45ac4242c32e3a7ba3a7f70b571bd0cc8ee4883985b8652af728cb74b` | `ghost-launcher/src/bin/v3_replay.rs` |
| `479d370849981b7b8789982a04246b37794779e451e5ef8f96c09398f0911373` | `ghost-launcher/src/components/gatekeeper_v3.rs` |
| `53fdd6eefe5965ccdef1ac0bad54ebbe0a35a1e7f4cec6ca606e35879f8e924c` | `ghost-launcher/src/components/iwim_veto.rs` |
| `331502d2ea26acb8b251a78bbbadfd4c729fe25961feae98f2d6ef40cc596eb0` | `ghost-launcher/src/components/live_tx_sender.rs` |
| `3e63606a4cd10d3c9a5d37fc377cb44c64d6e4a9c754ffe761020b246589c13a` | `ghost-launcher/src/components/post_buy_runtime.rs` |
| `bf41696d10f22c242d5edeef5f906dd9ae8910d6dace527422a86877b9bdb496` | `off-chain/components/trigger/src/jito_client.rs` |
| `e14966eb520d2bf3fbc9f52321125cd20d37e5c3d966b3a97bf98aa54dab736f` | `ghost-brain/src/pipeline/execution.rs` |
| `986730b2ca5c4d98c3d36043b184b9ee9c4c16f5ebc7890b85d0da119806acb9` | `ghost-brain/src/pipeline/jito_processor.rs` |

PR2B nie rozpoczyna PR2C, PR3 ani Type-5 T1. Rollout pozostaje `Legacy`; nie
aktywuje `DualCompute` ani V2.

## 10. Markery końcowe

```text
PR2B_FLIP_V2_STATE_MACHINE_PASS
PR2B_MANIPULATION_PRESENCE_AWARE_EVIDENCE_PASS
PR2B_RESERVE_VELOCITY_TYPED_EVIDENCE_PASS
PR2B_RECENT_BUY_SELL_TYPED_EVIDENCE_PASS
PR2B_EFFECTIVE_CONFIG_KEY_COVERAGE_CLOSED
PR2B_ONE_PRODUCER_ONE_SNAPSHOT_TWO_REPRESENTATIONS_PASS
PR2B_MFS_ATOMIC_PROJECTION_MATERIALIZATION_PASS
PR2B_PROJECTION_RESOURCE_GATE_PASS
GATEKEEPER_POLICY_UNCHANGED
V3_V1_REPLAY_UNCHANGED
TYPE5_NOT_STARTED
METRIC_CONTRACTS_V1_1_EVIDENCE_PRODUCERS_READY
PR2B_READY_FOR_REVIEW
```
