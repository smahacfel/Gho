# PR2C durable metric-contract evidence, replay i audit — raport weryfikacyjny

Status: `IMPLEMENTED / LOCALLY VERIFIED / READY FOR DRAFT PR REVIEW`

Data: 2026-07-13

## 1. Git i zakres

```text
repository: smahacfel/Gho
branch: agent/metric-contract-pr2c-durable-evidence-replay
base: fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9
merge-base: fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9
publication head: commit containing this report; authoritative SHA is the PR head
commit message: metric-contracts: add PR2C durable evidence replay
```

SHA commita nie może być wpisany do payloadu tego samego commita bez
samoodwołującego się hasha. Autorytatywny head jest dlatego publikowany w
metadanych draft PR i końcowym raporcie wykonawczym, a ten dokument zamraża
base, branch, message i dokładny tree scope.

Warunek wejściowy:

```text
git merge-base --is-ancestor \
  6348896ba303e9fb6dfb6c3bf2c5f9c015bf2c8e \
  origin/main
exit 0 — PASS
```

PR2C rozpoczęto w osobnym worktree z czystego `origin/main`. Nie zmodyfikowano
zastanych zmian użytkownika w głównym checkout. Rollout pozostaje `Legacy`.

## 2. Dokładna allowlista plików

Zmodyfikowane:

```text
ghost-brain/Cargo.toml
ghost-brain/src/oracle/decision_logger.rs
ghost-brain/src/oracle/mod.rs
ghost-core/Cargo.toml
ghost-core/src/metric_contracts/effective_config.rs
ghost-core/src/metric_contracts/evidence.rs
ghost-core/src/metric_contracts/mod.rs
ghost-core/src/metric_contracts/projection.rs
ghost-launcher/Cargo.toml
ghost-launcher/src/metric_contracts/mod.rs
ghost-launcher/src/metric_contracts/pr2b.rs
ghost-launcher/src/oracle_runtime.rs
ghost-launcher/src/session/observation.rs
ghost-launcher/tests/metric_contracts_pr2b_static_guards.rs
ghost-launcher/tests/refactor_invariants_tests.rs
```

Nowe:

```text
docs/ADR/ADR_8D_PR2C_METRIC_CONTRACT_DURABLE_EVIDENCE_REPLAY_20260713.md
ghost-brain/build.rs
ghost-brain/src/oracle/metric_contract_writer.rs
ghost-core/src/metric_contracts/pr2c.rs
ghost-launcher/src/bin/metric_contract_audit.rs
ghost-launcher/src/metric_contracts/pr2c.rs
ghost-launcher/src/metric_contracts/pr2c_audit.rs
ghost-launcher/src/metric_contracts/pr2c_replay.rs
ghost-launcher/tests/common/metric_contracts_pr2c.rs
ghost-launcher/tests/metric_contracts_pr2c_audit.rs
ghost-launcher/tests/metric_contracts_pr2c_comparator.rs
ghost-launcher/tests/metric_contracts_pr2c_durability.rs
ghost-launcher/tests/metric_contracts_pr2c_replay.rs
reports/metric_contracts/BURN_IN_CONTRACT_V1.json
reports/metric_contracts/historical_feasibility_post_pr2c_v1.md
reports/metric_contracts/metric_contract_wire_v1_schema_manifest.json
reports/metric_contracts/pr2c_durable_evidence_replay_verification_v1.md
```

Staging jest wykonywany wyłącznie dla tej jawnej listy. Dwa machine-readable
goldens są świadomie force-added, ponieważ katalog raportów ma regułę ignore.
Nie użyto `git add .`.

## 3. Wire V1 codebook manifest

```text
wire_version: 1
tuple_layout_tables: 18
enum_reason_mapping_tables: 28
BLAKE3: 70d79931f3f9a82720e46f622d439930a087431e305d14c02d88dcd26568fc7f
```

Golden:
`reports/metric_contracts/metric_contract_wire_v1_schema_manifest.json`.

Test sprawdza exact ordered equality z bieżącym codebookiem oraz sensitivity na
zmianę wire version, kolejności i nazwy tabeli, pozycji, kodu i wpisu. Replay v2
sprawdza ten sam frozen hash przed każdym rebuildem. Golden pojedynczego Wire V1
payloadu nadal istnieje, lecz nie zastępuje zamkniętego manifestu codebooku.

## 4. One frozen snapshot proof

`PoolObservationSession::try_materialize_features()` wykonuje canonical family
producers raz i tworzy jeden timed wrapper wokół niezmienionego
`Pr2bCompleteMetricContractSnapshotV1`. Compact projection zostaje atomowo
przypisana do MFS, a pełny snapshot jest przechowywany poza MFS i konsumowany
jednorazowo przez terminalny path PR2C.

Terminal sprawdza exact equality projection w assessment MFS z projection
snapshotu. `DecisionSnapshotMismatch` kończy zapis fail-closed. Typed proofy
`MetricDecisionProjectionValidatedContextV1` i
`MetricDecisionProjectionValidatedInputsV1` pozwalają zweryfikować profile,
effective-config i full evidence raz, bez osłabienia publicznego
`validated_canonical_hash(context)` dla arbitralnych callerów.

Nie występuje:

- drugi family producer call;
- full evidence w MFS;
- reconstruction full evidence z projection;
- raw/live read w projection builderze, replayu lub comparatorze;
- session/MFS/producer lock przez queue await lub filesystem I/O.

## 5. Compact v34 i durable sidecar

Exact v34 field-set test wymaga piętnastu zatwierdzonych pól i odrzuca unknown.
`GatekeeperBuyLog` zachowuje schema constant `33`, a exact wycinek definicji
struktury ma identyczny SHA-256 z base:

```text
2c351ad2ebddfba6a6b2597b3f8f38af42648c1dbe3cada64a5a540ce7bc42f7
```

Sidecar `metric_contract_evidence_v1.jsonl` używa
`MetricContractEvidenceTransportV1`. Konstruktor i deserializer wykonują
semantic/profile/hash validation. `evidence_sha256` jest canonical SHA-256
semantic payloadu bez writer timestamp i rotation index. Hash mismatch,
unknown field, partial evidence i unsupported schema są odrzucane.

## 6. Paired writer i rotation

Jeden `LogCommand::WriteMetricContractPair` trafia do istniejącej bounded queue.
Writer rozdziela summary i evidence na dwa pliki bez twierdzenia o filesystem
atomicity. Manifest obejmuje wymagane liczniki, bounded histograms, parts i
provenance. Pole `writer_finalized` staje się `true` wyłącznie po zamknięciu
writer task; audit odrzuca manifest pozostający mutable.

Regresje obejmują:

- normalny pair i 128 rzeczywistych enqueue przez `DecisionLogger`;
- summary ENOSPC;
- evidence ENOSPC po summary;
- disabled writer;
- channel close/send failure/drop;
- bounded queue high-water;
- partial write i utrwalenie failure manifestu;
- truncated JSONL;
- zmieniony part SHA;
- brakujący i nadmiarowy part;
- nieciągły part index;
- missing/orphan pair i duplicate full identity.

## 7. Identity i join contract

```text
record identity = (run_id, join_key, decision_plane)
stable event identity = pool creation transaction source signature, when present
```

Ten sam join key w różnych runach nie jest duplicate. Ta sama stable source
signature w niepokrywających się runach jest osobnym collision failure. Brak
stable identity daje `NOT_EVALUABLE`, nigdy zero collisions. Stable identity
nie jest wyprowadzana z join key.

## 8. Replay v1 i replay v2

V3 replay v1 pozostaje niezmieniony i nie importuje evidence transportu. MFS
bez historycznego Wire V1 pola nadal daje `None`.

Replay v2 weryfikuje manifest i part SHA w warstwie audytu, evidence transport
hash, full identity, profile, effective-config i cutoff. Następnie odbudowuje
projection tylko z full evidence, wymaga exact domain equality z decision-time
MFS projection, identycznego semantic projection hash i poprawnego Wire V1
round-trip. Unknown/partial schema i każdy context mismatch są fail-closed.

## 9. Comparator matrix

| Lane | Exact porównanie | Drift |
| --- | --- | --- |
| verdict | typed normalized verdict | `FAIL_POLICY_DRIFT` |
| primary reason | stable typed reason code | `FAIL_POLICY_DRIFT` |
| reason chain | stable ordered identity | `FAIL_POLICY_DRIFT` |
| phases | six-element pass vector | `FAIL_POLICY_DRIFT` |
| soft points | exact integer | `FAIL_POLICY_DRIFT` |
| selector | exact score | `FAIL_POLICY_DRIFT` |
| hard fail | exact classification | `FAIL_POLICY_DRIFT` |
| dev-primary | semantic counterfactual | diagnostic only |
| corrected FTDI | semantic counterfactual | diagnostic only |

Runtime comparator wykonuje rzeczywiste drugie
`evaluate_policy_from_assessment()` na tym samym frozen assessment/config.
Nie porównuje obiektu z jego klonem, nie czyta live state, nie uruchamia IWIM
ani execution i nie emituje drugiego terminal eventu.

## 10. Single-run, bundle i BURN_IN_CONTRACT_V1

CLI terminal classes:

```text
PASS_CUTOVER_READY
NOT_EVALUABLE
FAIL_SCHEMA_OR_REPLAY
FAIL_POLICY_DRIFT
FAIL_RESOURCE_BUDGET
```

Bundle agreguje minima dopiero po per-run PASS. Frozen contract:

```text
owner approval identity: github:smahacfel:authorized-pr2c-task:2026-07-13
canonical SHA-256: 56ceb5a80a0b6d413cf639f0ac02d30fade2770f7f5c4cf4a1a014f3632ae7df
```

Rows niepóźniejsze niż `frozen_at` nie są prospective validation evidence.
Zmiana któregokolwiek gate’u wymaga nowego contract version/hash/freeze i nie
może retroaktywnie zaliczyć rows starego contractu.

## 11. Resource measurements

Normatywny release harness mierzy pełny production path:

```text
full evidence
→ projection build
→ wszystkie family/root semantic validations
→ Wire V1 hard-size gate
→ canonical semantic hash
→ v34 + evidence pair
→ exact JSON serialization
→ real frozen policy comparator
```

Komenda:

```text
cargo test --release -p ghost-launcher \
  --test metric_contracts_pr2c_durability \
  pr2c_release_resource_harness_reports_full_path_percentiles \
  -- --nocapture
```

Uruchomiono 1 test, 0 failures; filtr nie był zero-testowym PASS.

| Metryka | p50 | p95 | p99/max | Limit |
| --- | ---: | ---: | ---: | ---: |
| metric_contract_build_and_serialize_us | 484 | 625 | 796 | p99 ≤ 1000 us |
| projection_build_validate_us | 558 | 732 | 919 | p99 ≤ 1000 us |
| pair_build_us | 442 | 550 | 734 | diagnostic |
| transport_serialize_us | 39 | 57 | 71 | diagnostic |
| wire_serialize_us | 54 | 74 | 89 | diagnostic |
| comparator_elapsed_us | 4 | 7 | 10 | p99 ≤ 1000 us |
| projection_wire_json_bytes | — | 2,339 | 2,339 max | p95 ≤ 12 KiB; max ≤ 16 KiB |
| sidecar_json_bytes | — | 21,406 | 21,406 | p95 ≤ 24 KiB; p99 ≤ 48 KiB |
| v34_json_bytes | — | 1,167 | — | oba v34 delta gates |

Rzeczywisty bounded queue test, 128 enqueue i capacity 1000:

```text
logger_enqueue_wait_us p99 upper bound: 32 us
writer_queue_high_water: 128 / 1000 = 12.8%
dropped rows: 0
writer failures: 0
orphan summary/evidence: 0
```

Frozen paired v33 p95 wynosi 119,890 B. v34 jest mniejsze o 118,723 B
(`-99.03%`), więc przechodzi jednocześnie limit +8 KiB i +10%. Dodatkowy
v34+sidecar p95 to 22,573 B, czyli 18.83% frozen v33 p95 i poniżej limitu 25%
combined GB/hour delta. Nie usunięto critical provenance dla uzyskania PASS.

Diagnostyczne p50/p95/p99, niewliczane drugi raz do acceptance path:

```text
profile hash: 140 / 172 / 219 us
effective-config hash: 318 / 395 / 437 us
public arbitrary-projection validated hash: 946 / 1140 / 1397 us
evidence transport construction: 588 / 695 / 968 us
pair validate: 324 / 408 / 542 us
```

Publiczny arbitrary-input hash pozostaje pełnym independently validated path.
Normatywny terminal path wykorzystuje typed proofs i mieści się w gate’ach.

## 12. Historical feasibility

Kontrolowany release CLI audit wykonano poleceniem opisanym dokładnie w
`historical_feasibility_post_pr2c_v1.md`. Zakończył się kodem 3 na braku
`metric_contract_rotation_manifest_v1.json`, przed odczytem v33. Lokalne raw
runy PR0 nie są już dostępne, dlatego nie wykonano ani nie zadeklarowano nowego
content scan. Frozen PR0 manifest/summary pozostaje dowodem feasibility scale.

Historyczne v33 nie ma paired artifacts, exact effective-config ani stable
identity i jest `NOT_EVALUABLE`; contribution do prospective counts wynosi 0.

## 13. Pełna macierz testów

| Komenda | Wynik |
| --- | --- |
| `cargo test -p ghost-core --test metric_contracts_v1_1_foundation` | PASS — 19/19 |
| `cargo test -p ghost-core --test metric_contracts_v1_1_projection` | PASS — 23/23 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2a_producers` | PASS — 26/26 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2a_static_guards` | PASS — 8/8 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2b_producers` | PASS — 16/16 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2b_static_guards` | PASS — 6/6 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2c_durability` | PASS — 12/12 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2c_replay` | PASS — 6/6 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2c_comparator` | PASS — 6/6 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2c_audit` | PASS — 9/9 |
| `cargo test -p ghost-launcher --test gatekeeper_policy_tests` | PASS — 46/46 |
| `cargo test -p ghost-launcher --test gatekeeper_v25_regression` | PASS — 42/42 |
| `cargo test -p ghost-launcher --test gatekeeper_v3_tests` | PASS — 9/9 |
| `cargo test -p ghost-launcher --test session_lifecycle_tests` | PASS — 26/26 |
| `cargo test -p ghost-launcher --test refactor_invariants_tests` | PASS — 12/12 |
| `cargo test -p ghost-brain --lib replay_payload` | PASS — 5/5 |
| release resource harness | PASS — 1/1 |
| bounded queue resource filter | PASS — 1/1 |
| `cargo check -p ghost-core` | PASS |
| `cargo check -p ghost-launcher` | PASS |
| `cargo check -p ghost-brain` | PASS |
| targeted Clippy changed crates/lib/tests/bins | PASS po wyłączeniu dwóch udowodnionych baseline lintów |
| `cargo fmt --all -- --check` | PASS po finalnym formatowaniu |
| `git diff --check` | PASS |
| `git diff --cached --check` | wykonywany po jawym stagingu |

Clippy bez wyjątków kończy się na dwóch niezmienionych powierzchniach:

```text
ghost-brain/src/pipeline/execution.rs:1569
  clippy::never_loop

ghost-brain/tests/mock_pump_amm.rs:185,206
  clippy::absurd_extreme_comparisons
```

Identyczny targeted scope przechodzi exit 0 z wyłączeniem wyłącznie tych dwóch
baseline lint classes. Istniejące warnings repo nie są przypisane PR2C.

## 14. Forbidden-scope proof

Każdy z poniższych plików ma pusty diff względem base i SHA-256:

```text
5ec6a766e8bb2d9b0cacdc39d7146b5fac056e0cf57042b75b4077a82fd9210c  ghost-launcher/src/components/gatekeeper_policy.rs
feca4ba45ac4242c32e3a7ba3a7f70b571bd0cc8ee4883985b8652af728cb74b  ghost-launcher/src/bin/v3_replay.rs
479d370849981b7b8789982a04246b37794779e451e5ef8f96c09398f0911373  ghost-launcher/src/components/gatekeeper_v3.rs
53fdd6eefe5965ccdef1ac0bad54ebbe0a35a1e7f4cec6ca606e35879f8e924c  ghost-launcher/src/components/iwim_veto.rs
331502d2ea26acb8b251a78bbbadfd4c729fe25961feae98f2d6ef40cc596eb0  ghost-launcher/src/components/live_tx_sender.rs
3e63606a4cd10d3c9a5d37fc377cb44c64d6e4a9c754ffe761020b246589c13a  ghost-launcher/src/components/post_buy_runtime.rs
bf41696d10f22c242d5edeef5f906dd9ae8910d6dace527422a86877b9bdb496  off-chain/components/trigger/src/jito_client.rs
e14966eb520d2bf3fbc9f52321125cd20d37e5c3d966b3a97bf98aa54dab736f  ghost-brain/src/pipeline/execution.rs
986730b2ca5c4d98c3d36043b184b9ee9c4c16f5ebc7890b85d0da119806acb9  ghost-brain/src/pipeline/jito_processor.rs
```

DecisionLogger jest celowo zmieniony tylko addytywnie dla paired command.
`GatekeeperBuyLog` struct SHA jest identyczny z base, schema constant pozostaje
33, a diff hunków kończy się przed selector-score regionem.

Potwierdzone:

- brak zmian Gatekeeper thresholds, weights, phases, soft points, verdicts i
  primary reasons;
- brak zmian V3 behavior i replay v1;
- brak zmian selector score, IWIM, sender, Jito, execution ani post-buy;
- authority Profile A i aktywny dev source bez zmian;
- rollout pozostaje `Legacy`; nie aktywowano DualCompute/V2;
- nie rozpoczęto PR3 ani Type-5 T1.

## 15. Znany baseline failure

```text
cargo test -p ghost-brain --lib selector_shadow_score
8 passed; 1 failed:
test_selector_shadow_score_filters_non_finite_feature_values
```

To wcześniejszy baseline. Wszystkie hunk headers PR2C w
`decision_logger.rs` kończą się na regionie paired writera (do bazowej linii
3115), podczas gdy selector helpers zaczynają się po bieżącej linii 3422, a
failing test po linii 6537. PR2C nie zmienia selector score ani tego testu.

## 16. Markery końcowe

```text
METRIC_CONTRACT_WIRE_V1_CODEBOOK_MANIFEST_FROZEN
PR2C_V34_COMPACT_SUMMARY_PASS
PR2C_PAIRED_FULL_EVIDENCE_SIDECAR_PASS
PR2C_RECORD_IDENTITY_AND_STABLE_EVENT_CONTRACT_PASS
PR2C_ROTATION_MANIFEST_SHA_PASS
PR2C_REPLAY_V1_COMPATIBILITY_PASS
PR2C_REPLAY_V2_PROJECTION_FULL_EQUALITY_PASS
PR2C_EQUIVALENCE_COMPARATOR_ZERO_DRIFT_PASS
PR2C_COUNTERFACTUAL_DIAGNOSTIC_PASS
PR2C_SINGLE_RUN_AUDIT_PASS
PR2C_BUNDLE_AUDIT_PASS
PR2C_RESOURCE_GATES_PASS
BURN_IN_CONTRACT_V1_FROZEN
GATEKEEPER_POLICY_UNCHANGED
V3_V1_REPLAY_UNCHANGED
TYPE5_NOT_STARTED
METRIC_CONTRACTS_V1_1_DUAL_COMPUTE_READY_FOR_PROSPECTIVE_BURN_IN
PR2C_READY_FOR_REVIEW
```
