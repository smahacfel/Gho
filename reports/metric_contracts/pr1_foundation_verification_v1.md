# PR1 metric-contract foundation — raport weryfikacyjny V1

Status: `VERIFICATION_COMPLETE / LEGACY_PARITY_PASS_WITH_ONE_UNRELATED_BASELINE_TEST_FAILURE`

Data: 2026-07-11

Repo: `/root/Gho_dynamic_exit_v1`

Branch: `agent/metric-contract-pr1-foundation`

Audytowany base:
`9ae1a30dd5b681f7777fa8e833e7103fde73a647`

Plan:
`PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`

ADR:
`docs/ADR/ADR_8D_PR1_METRIC_CONTRACT_REGISTRY_PROFILE_STATUS_FOUNDATION_20260711.md`

## 1. Zakres dowodu

Raport rozdziela pięć pytań:

1. Czy registry/profile/status/hash/identity schemas są wewnętrznie kompletne i
   fail-closed?
2. Czy stary i bieżący TOML zachowują Legacy + Profile A?
3. Czy PR1 nie aktywuje dual compute, v34, replay v2 ani nowych producerów?
4. Czy istniejący top3 preferred-plus-fallback selector zachowuje parity?
5. Czy Gatekeeper verdict/reason/phase/soft-points oraz selector path pozostały
   poza zakresem zmian?

Raport nie stanowi producer/replay/burn-in evidence. Te poziomy należą do
PR2A/PR2B/PR2C i prospective validation.

## 2. Foundation test matrix

Komenda:

```bash
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test -p ghost-core --test metric_contracts_v1_1_foundation
```

Wynik: `15 passed; 0 failed`.

Pokrycie:

- dokładnie 10 contract IDs i 32 unikalne surfaces;
- Profile A completeness i canonical order;
- profile hash wiąże authority entries oraz pełny semantic registry snapshot;
- zmieniony population/denominator registry jest odrzucany;
- non-policy classes nigdy nie stają się authoritative;
- legacy authority pozostaje identyczne w DualCompute;
- V2 promotion tylko dla `EquivalentCutover`;
- deterministic profile hash i sensitivity każdego authority entry;
- RFC 8785 object/Unicode/number rules;
- `-0`, exponent formatting, NaN i infinity;
- required `null` kontra omitted key;
- canonical `u64`, `u128` i `i64` decimal strings;
- semantic payload bez self-hash;
- closed/sorted effective config;
- missing/duplicate/wrong-kind/hash tamper rejection;
- exhaustive effective-config leaf hash mutation;
- record identity kontra stable event identity;
- wszystkie 32 envelope slots i evidence transport hash;
- one-known-source legacy FSC = unavailable/null;
- flip owner/state/aggregate completeness;
- reserve/recent/manipulation negative invariants;
- exhaustive legacy status adapters;
- typed reason-family mismatch rejection;
- measured zero kontra missing manipulation raw field.

## 3. Config i startup compatibility

Komendy:

```bash
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test -p ghost-brain metric_contract_foundation -- --nocapture

CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test -p ghost-brain --lib \
  current_repository_toml_resolves_legacy_profile_a_without_edit

CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test -p ghost-launcher --bin ghost-launcher \
  metric_contract_pr1_startup_defaults_to_legacy_and_rejects_known_nonlegacy_modes
```

Wyniki:

```text
old TOML defaults                         PASS
unknown mode/profile rejected             PASS
current repository TOML without edit      PASS
missing config defaults Legacy/Profile A  PASS
known DualCompute/V2 rejected in PR1       PASS
```

Launcher loguje foundation mode, profile ID i profile hash. Nie loguje
fałszywego resolved effective-config hash przed związaniem rzeczywistych
producer values.

## 4. Top3 reconciliation i parity

Komendy:

```bash
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test -p ghost-launcher --test gatekeeper_policy_tests top3

CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test -p ghost-launcher --test gatekeeper_v25_regression \
  p4_top3_signer_ratio_uses_ratio_and_pdd_percent_scale
```

Wynik: `4 passed; 0 failed`.

Udowodniono:

- preferred value ma pierwszeństwo;
- brak preferred field korzysta bit-for-bit z legacy aliasu;
- alias pozostaje ratio-scale 0..1;
- PDD percent conversion nie zmienia skali;
- istniejący helper nie został zduplikowany.

Static guard:

```bash
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test -p ghost-launcher --test refactor_invariants_tests
```

Wynik: `12 passed; 0 failed`.

Guard sprawdza aktywne materialization, Gatekeeper policy i assessment/log
adapters oraz zamrożenie v33/V3-v1.

## 5. Gatekeeper legacy parity

Komendy:

```bash
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test -p ghost-launcher --test gatekeeper_policy_tests

CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test -p ghost-launcher --test gatekeeper_v25_regression
```

Wyniki:

```text
Gatekeeper V2 policy integration:  46 passed; 0 failed
Gatekeeper V2.5 regressions:        42 passed; 0 failed
```

Ponadto `git diff --exit-code origin/main` zwraca 0 dla:

- `ghost-launcher/src/components/gatekeeper_policy.rs`;
- `ghost-launcher/src/components/gatekeeper.rs`;
- `ghost-core/src/checkpoint/types.rs`.

Wniosek: PR1 nie zmienił policy source, MFS layout, thresholdów, faz,
verdict/reason taxonomy ani soft-point path.

## 6. DecisionLogger i replay freeze

Komendy:

```bash
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test -p ghost-brain --lib test_gatekeeper_buy_log_file_write

CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test -p ghost-brain --lib replay_payload
```

Wyniki:

```text
v33 DecisionLogger write:      1 passed; 0 failed
V3 replay compatibility:       5 passed; 0 failed
```

Static/source proof:

```text
GATEKEEPER_BUY_LOG_SCHEMA_VERSION = 33
SUPPORTED_REPLAY_PAYLOAD_SCHEMA_VERSION = 1
MetricContractDecisionSummaryV1 absent from DecisionLogger
MetricContractEvidenceTransportV1 absent from v3_replay
```

`git diff --exit-code origin/main` jest pusty dla DecisionLogger i v3 replay.

## 7. Selector sweep i baseline failure classification

Komenda:

```bash
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo test -p ghost-brain --lib selector_shadow_score
```

Wynik:

```text
8 passed
1 failed: test_selector_shadow_score_filters_non_finite_feature_values
```

Failure nie jest PR1 regression:

```text
working-tree blob = 790db5300db39036a6309552d75578ebeac2aa1c
origin/main blob  = 790db5300db39036a6309552d75578ebeac2aa1c
git diff origin/main -- ghost-brain/src/oracle/decision_logger.rs = empty
```

Istniejący test oczekuje `gk_vector_price_first` i
`gk_vector_price_return` w missing vector, gdy pierwszy element tablicy jest
`None`. Istniejąca implementacja `selector_shadow_price_first()` filtruje
missing/non-finite entries i wybiera pierwszy finite price; return używa tej
samej finite listy. Test i implementacja są więc niespójne już na base.

PR1 nie zmienia selector mapping/model/score/logger, dlatego naprawa tego
kontraktu jest jawnie poza zakresem. Failure jest zapisany, nie ukryty ani
przedstawiony jako PASS.

## 8. Build, format i lint

Komendy i wyniki:

```text
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo check -p ghost-launcher
  PASS

cargo fmt --all -- --check
  PASS

CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 \
  cargo clippy -p ghost-core --test metric_contracts_v1_1_foundation --no-deps
  PASS

git diff --check
  PASS
```

Clippy raportuje istniejące warnings poza `metric_contracts/*`; targeted run
kończy się kodem 0 i nie raportuje nowego foundation warning.

## 9. Scope audit

PR1-owned tracked/new paths:

```text
ghost-core/Cargo.toml
ghost-core/src/lib.rs
ghost-core/src/metric_contracts/*.rs
ghost-core/tests/metric_contracts_v1_1_foundation.rs
ghost-brain/src/config/ghost_brain_config.rs
ghost-brain/src/config/mod.rs
ghost-launcher/src/main.rs
ghost-launcher/src/session/observation.rs
ghost-launcher/tests/refactor_invariants_tests.rs
docs/ADR/ADR_8D_PR1_METRIC_CONTRACT_REGISTRY_PROFILE_STATUS_FOUNDATION_20260711.md
reports/metric_contracts/pr1_foundation_verification_v1.md
```

Niezwiązane dirty/untracked pliki użytkownika istnieją w worktree, ale nie są
częścią powyższego allowlist i nie były modyfikowane przez PR1.

Forbidden-scope diff:

```text
Gatekeeper policy/engine                 empty
MaterializedFeatureSet/checkpoint types  empty
DecisionLogger                           empty
V3 replay                                empty
Gatekeeper V3 config                     empty
execution/sender/post-buy                empty
```

## 10. Werdykt

```text
REGISTRY_10_CONTRACTS_32_SURFACES_PASS
PROFILE_A_AUTHORITY_CEILING_PASS
CANONICAL_HASH_V1_PASS
EFFECTIVE_CONFIG_SCHEMA_PASS
IDENTITY_SEPARATION_PASS
CANONICAL_STATUS_AND_ADAPTERS_PASS
SHARED_EVIDENCE_SCHEMA_PASS
OLD_TOML_COMPATIBILITY_PASS
V33_V3V1_FREEZE_PASS
GATEKEEPER_LEGACY_PARITY_PASS
TOP3_SELECTOR_PARITY_PASS
SELECTOR_SOURCE_UNCHANGED_PASS
METRIC_CONTRACTS_V1_1_FOUNDATION_LEGACY_PARITY
```

Następny dozwolony milestone po review: `PR2A`. PR2B/PR2C/PR3 nie są przez ten
raport automatycznie odblokowane.
