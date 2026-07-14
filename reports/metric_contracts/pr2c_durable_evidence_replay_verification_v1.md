# PR2C durable metric-contract evidence, replay i audit — raport weryfikacyjny

Status: `FINAL ISOLATION DIRECTIVE IMPLEMENTED / LOCAL FULL VALIDATION PASS`

Data: 2026-07-13

## 1. Git i zakres

```text
PR: #65 — PR2C: add durable metric-contract evidence, replay and audit
branch: agent/metric-contract-pr2c-durable-evidence-replay
base: fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9
merge-base: fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9
ostatni zdalny head przed finalną dyrektywą: 261964d751c4356d14f6ad06df6e729524597bba
lokalny head wejściowy finalizacji: aa2035d333113632bcb7bf17f3a31bcbcf0bee20
publication head: commit zawierający ten raport; authoritative SHA = head PR
rekomendowany commit message: metric-contracts: isolate PR2C durable evidence runtime
```

PR pozostaje draftem. Nie rozpoczęto PR3 ani Type-5 T1. Prospective burn-in
nie jest autoryzowany przez ten PR.

Finalna dyrektywa zmieniła kryterium zakończenia: durable latency
producer-to-fsync nie jest już merge gate'em. Kryterium runtime stanowi
fizyczna izolacja PR2C, domyślne wyłączenie, nieblokujący enqueue oraz
fail-closed invalidation dedykowanego evidence runu bez wpływu na Gatekeeper
i v33.

## 2. Dokładna allowlista całego PR względem base

PR obejmuje dokładnie poniższe pliki:

```text
.github/workflows/metric-contracts-pr2c.yml
PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md
docs/ADR/ADR_8D_PR2C_METRIC_CONTRACT_DURABLE_EVIDENCE_REPLAY_20260713.md
docs/ADR/ADR_8D_PR2C_REVIEW_BLOCKERS_DURABILITY_AUDIT_20260713.md
docs/ADR/ADR_8D_PR2C_SECOND_REVIEW_DURABLE_EQUIVALENCE_RESOURCE_INTEGRITY_20260713.md
ghost-brain/Cargo.toml
ghost-brain/build.rs
ghost-brain/examples/oracle_decision_dry_run.rs
ghost-brain/src/config/ghost_brain_config.rs
ghost-brain/src/oracle/decision_logger.rs
ghost-brain/src/oracle/followup_scoring.rs
ghost-brain/src/oracle/metric_contract_writer.rs
ghost-brain/src/oracle/mod.rs
ghost-brain/tests/oracle_decision_logger_integration.rs
ghost-core/Cargo.toml
ghost-core/src/metric_contracts/canonical_hash.rs
ghost-core/src/metric_contracts/effective_config.rs
ghost-core/src/metric_contracts/evidence.rs
ghost-core/src/metric_contracts/mod.rs
ghost-core/src/metric_contracts/pr2c.rs
ghost-core/src/metric_contracts/projection.rs
ghost-core/tests/metric_contracts_v1_1_foundation.rs
ghost-core/tests/metric_contracts_v1_1_projection.rs
ghost-launcher/Cargo.toml
ghost-launcher/src/bin/metric_contract_audit.rs
ghost-launcher/src/main.rs
ghost-launcher/src/metric_contracts/mod.rs
ghost-launcher/src/metric_contracts/pr2a.rs
ghost-launcher/src/metric_contracts/pr2b.rs
ghost-launcher/src/metric_contracts/pr2c.rs
ghost-launcher/src/metric_contracts/pr2c_audit.rs
ghost-launcher/src/metric_contracts/pr2c_replay.rs
ghost-launcher/src/oracle_runtime.rs
ghost-launcher/src/session/manager.rs
ghost-launcher/src/session/observation.rs
ghost-launcher/tests/common/metric_contracts_pr2c.rs
ghost-launcher/tests/metric_contracts_pr2b_static_guards.rs
ghost-launcher/tests/metric_contracts_pr2c_audit.rs
ghost-launcher/tests/metric_contracts_pr2c_comparator.rs
ghost-launcher/tests/metric_contracts_pr2c_durability.rs
ghost-launcher/tests/metric_contracts_pr2c_replay.rs
ghost-launcher/tests/refactor_invariants_tests.rs
reports/metric_contracts/BURN_IN_CONTRACT_V2.json
reports/metric_contracts/historical_feasibility_post_pr2c_v1.md
reports/metric_contracts/metric_contract_wire_v1_schema_manifest.json
reports/metric_contracts/pr2c_durable_evidence_replay_verification_v1.md
```

Finalny amendment względem zdalnego heada `261964d7...` jest ograniczony do:

```text
.github/workflows/metric-contracts-pr2c.yml
ghost-brain/examples/oracle_decision_dry_run.rs
ghost-brain/src/config/ghost_brain_config.rs
ghost-brain/src/oracle/decision_logger.rs
ghost-brain/src/oracle/followup_scoring.rs
ghost-brain/src/oracle/metric_contract_writer.rs
ghost-brain/src/oracle/mod.rs
ghost-brain/tests/oracle_decision_logger_integration.rs
ghost-launcher/src/main.rs
ghost-launcher/src/metric_contracts/pr2c_audit.rs
ghost-launcher/src/oracle_runtime.rs
ghost-launcher/tests/common/metric_contracts_pr2c.rs
ghost-launcher/tests/metric_contracts_pr2c_comparator.rs
ghost-launcher/tests/metric_contracts_pr2c_durability.rs
ghost-launcher/tests/refactor_invariants_tests.rs
reports/metric_contracts/pr2c_durable_evidence_replay_verification_v1.md
```

Nie utworzono nowego planu, ADR-u ani amendment frameworku. Zmiany lokalne z
commitów `c1f28f4` (cache uchwytów v33) i `aa2035d` (izolacja próbek starego
benchmarku) zostały wycofane. BURN V3 i histogram codebook V2 nie należą do
publikowanego diffu; zachowany BURN V2 nie autoryzuje prospective runu po tej
finalizacji.

## 3. Finalna architektura runtime

```text
Gatekeeper / terminal decision
  └─ istniejący DecisionLogger v33 queue/task → v33 JSONL

PR2C switch == true AND execution mode == Shadow
  └─ second compute + pair builder
     └─ try_send → osobna bounded PR2C queue
        └─ osobny PR2C writer task → v34 + evidence + manifest
```

`LogCommand`, który zasila istniejącą ścieżkę v33, nie zawiera paira PR2C.
`MetricContractLogCommand` jest odrębnym typem, ma odrębny kanał i odrębnego
consumera. `writer.write_pair(pair).await`, `sync_data` i finalizacja manifestu
wykonują się wyłącznie w PR2C tasku i nie mogą powodować head-of-line blocking
w tasku v33.

PR2C enqueue używa wyłącznie:

```text
tx.try_send(MetricContractLogCommand::WritePair(pair))
```

Nie używa `reserve().await` ani asynchronicznego oczekiwania na capacity.
`Full` i `Closed` zwracają typed `MetricContractEnqueueErrorV1`, zwiększają
oddzielne liczniki oraz ustawiają `evidence_run_invalid = true`. Awaria tej
gałęzi nie zmienia verdictu, reasonów, phases, soft points ani wyniku v33.

## 4. Switch OFF-by-default i shadow-only

Jedyny switch:

```text
GhostBrainConfig.metric_contract_pr2c_enabled: bool
serde default: false
```

Pole nie jest dopisane do checked-in `ghost_brain_config.toml`, dzięki czemu
historyczny raw config hash i v33 provenance pozostają niezmienione. Stary TOML
bez pola ładuje `false`; `true` musi zostać wpisane jawnie w dedykowanej kopii
konfiguracji evidence runu.

Launcher dodatkowo wymaga `ExecutionMode::Shadow`. `Live`, `Paper` i `Dual`
zawsze wymuszają OFF nawet przy `requested=true` i emitują warning. Rollout
metric contracts nadal pozostaje `Legacy`; nie aktywowano DualCompute ani V2.

Przy OFF:

- nie jest uruchamiany drugi policy compute;
- nie jest wywoływany PR2C pair builder;
- frozen PR2C snapshot nie jest konsumowany przez terminal pair path;
- nie istnieje PR2C sender/receiver ani writer task;
- nie ma PR2C enqueue;
- nie są otwierane pliki v34, evidence ani manifest;
- próba bezpośredniego pair enqueue zwraca `WriterDisabled` bez zmiany stats;
- exact serde bytes routed v33 dla `legacy_live` i `v25_shadow` są zgodne z
  dotychczasową serializacją.

## 5. Routing i identity

Zachowano typed routing poprawiony w poprzednim amendmentcie. Canonical plane
expansion/hydration pozostaje własnością DecisionLoggera. Pair przy ON pobiera
typed `Pr2cRoutedDecisionContextV1` z legacy-live row, obejmujący:

```text
(run_id, join_key, decision_plane)
gatekeeper_config_hash
brain_config_hash
```

Realny terminalny test zaczyna się od `GatekeeperAssessment::to_buy_log()` z
nieuzupełnionymi polami routingu i kończy exact trójstronnym joinem:

```text
v33 identity == v34 identity == evidence identity
```

Następnie finalizuje manifest i wykonuje single-run audit. Nie używa fixture'a
z ręcznie wypełnionym routed identity.

## 6. Zachowane durable i replay contracts

Zachowano bez redukcji:

- typed full evidence wszystkich dziesięciu rodzin;
- compact projection w MFS i pełne evidence poza MFS;
- v34 o dokładnym zatwierdzonym field-set;
- semantic evidence SHA-256 i niezależny durable source cutoff;
- Wire V1 manifest: 18 layouts, 28 mapping tables i frozen codebook hash;
- exact projection/full-evidence equality;
- replay v2: evidence hash, rebuilt projection/hash, Wire round-trip,
  projection-derived v34 fields i durable comparator deltas;
- rzeczywisty second compute comparator;
- persisted policy drift klasyfikowany jako `FAIL_POLICY_DRIFT`;
- counterfactual diagnostics dla dev-primary i corrected FTDI;
- writer fault/orphan/truncation detection i fsync/finalization semantics;
- single-run audit i opcjonalny offline bundle audit;
- replay v1 i historyczny brak projection jako `None`.

## 7. Queue/failure contract

Manifest/stats zawierają osobno:

```text
queue_full_total
queue_closed_total
queue_send_failures_total
queue_dropped_rows_total
writer_disabled_total
summary_write_failures_total
evidence_write_failures_total
orphan_summary_total
orphan_evidence_total
missing_pair_total
manifest_write_failures_total
finalization_failures_total
evidence_run_invalid
writer_queue_high_water
```

Każdy queue admission, write, pairing, manifest lub finalization fault
unieważnia evidence run. Audit nie może zwrócić PASS dla takiego manifestu.

Deterministyczna regresja saturacji używa capacity `1` na current-thread
runtime:

```text
first PR2C try_send  → accepted, queue pełna
second PR2C try_send → typed QueueFull, bez await
v33 enqueue/write    → PASS na osobnej kolejce
manifest             → finalized + evidence_run_invalid=true
audit                → FAIL_RESOURCE_BUDGET na clean build
```

Regresja zamkniętego kanału osobno wymaga `ChannelClosed` i typed invalidation.
Poprawny 128-row evidence run wymaga high-water poniżej 80%, zero full/closed,
zero drops, zero write failures, zero orphanów oraz `evidence_run_invalid=false`.

## 8. Resource semantics

Nie istnieje merge gate `producer → fsync → manifest <= 5 ms`. Audit nie
klasyfikuje runu na podstawie full durable latency p99. Pozostają correctness
i bounded-storage gates oraz queue completeness/failure gates.

Release harness raportuje diagnostycznie:

```text
metric_contract_build_and_serialize_us p50/p95/p99/max
snapshot/evidence/projection/pair substeps p50/p95/p99
comparator p50/p95/p99
Wire/v34/sidecar sizes
```

Nie dopisuje sztucznego enqueue sample. Workflow job jest nazwany
`release-resource-diagnostic`, nie resource gate. Histogram V1 pozostaje
diagnostyką; nie rozszerzono go wyłącznie po to, by spełniał dawny próg.

Finalny lokalny release diagnostic wykonał `16` warmup i `200` measured
samples. Histogram V1 umieszcza pełną ścieżkę powyżej `2 000 us` w bounded
overflow i dlatego konserwatywnie raportuje observed `max_us` jako
p50/p95/p99; te wartości nie są merge gate'em:

| Diagnostyka release | p50 | p95 | p99 | max |
| --- | ---: | ---: | ---: | ---: |
| pełny durable path | 14 594 us | 14 594 us | 14 594 us | 14 594 us |
| complete snapshot build+validate | 1 478 us | 1 896 us | 2 490 us | n/a |
| projection build+validate | 453 us | 575 us | 752 us | n/a |
| pair construction | 381 us | 507 us | 638 us | n/a |
| final serde substep | 45 us | 65 us | 80 us | n/a |
| comparator | 34 us | 83 us | 95 us | n/a |

Rozmiary: Wire V1 p95/max `2 339 B`, sidecar p95/p99 `22 242 B`, v34 p95
`1 177 B`. Oddzielny 128-row queue diagnostic dał `try_send` p99 `32 us` i
high-water `128/1000 = 12.8%`, przy zerowych drops/failures/orphans.

## 9. Test matrix finalnego amendmentu

| Komenda / regresja | Wynik |
| --- | --- |
| `cargo test -p ghost-core --test metric_contracts_v1_1_foundation` | PASS — 19/19 |
| `cargo test -p ghost-core --test metric_contracts_v1_1_projection` | PASS — 24/24 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2a_producers` | PASS — 26/26 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2a_static_guards` | PASS — 8/8 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2b_producers` | PASS — 16/16 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2b_static_guards` | PASS — 6/6 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2c_durability` | PASS — 21/21 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2c_replay` | PASS — 10/10 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2c_comparator` | PASS — 8/8 |
| `cargo test -p ghost-launcher --test metric_contracts_pr2c_audit` | PASS — 26/26 |
| disabled-mode exact v33 bytes / no artifacts | PASS — część durability 21/21 |
| real terminal routing E2E | PASS — część durability 21/21 |
| isolated queue saturation/closed | PASS — część durability 21/21 |
| 128-row queue high-water / zero failures | PASS — część durability 21/21 |
| `cargo test -p ghost-brain --lib metric_contract_pr2c_switch_is_backward_compatible_and_opt_in` | PASS — 1/1 |
| `cargo test -p ghost-launcher --lib pr2c_durable_evidence_is_opt_in_and_shadow_only` | PASS — 1/1 |
| `cargo test -p ghost-launcher --test gatekeeper_policy_tests` | PASS — 46/46 |
| `cargo test -p ghost-launcher --test gatekeeper_v25_regression` | PASS — 42/42 |
| `cargo test -p ghost-launcher --test gatekeeper_v3_tests` | PASS — 9/9 |
| `cargo test -p ghost-launcher --test session_lifecycle_tests` | PASS — 26/26 |
| `cargo test -p ghost-launcher --test refactor_invariants_tests` | PASS — 12/12 |
| `cargo test -p ghost-brain --lib replay_payload` | PASS — 5/5 |
| `cargo check -p ghost-core` | PASS |
| `cargo check -p ghost-launcher` | PASS |
| `cargo check -p ghost-brain` | PASS |
| targeted Clippy: core/launcher/brain | PASS — exit 0; istniejące warnings zachowane |
| `cargo check -p ghost-brain --examples` | PASS |
| `cargo test -p ghost-brain --test oracle_decision_logger_integration` | PASS — 4/4 |
| release durable latency diagnostic | PASS — 1/1; 16 warmup + 200 measured; wynik diagnostyczny, nie gate |
| `cargo fmt --all -- --check` | PASS |
| `git diff --check` | PASS |
| `git diff --cached --check` | PASS po jawnym stagingu |

Filtr uruchamiający zero testów nie jest uznawany za PASS. Jedyny użyty wyjątek
Clippy to istniejący, zamrożony `clippy::never_loop` w execution path; PR2C nie
zmienia tego pliku.

## 10. Forbidden-scope proof

Poniższe pliki mają pusty diff względem base oraz SHA-256:

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

Potwierdzone:

- brak zmian Gatekeeper thresholds, weights, phases, soft points, verdicts i
  primary reasons;
- brak zmian Gatekeeper V3 behavior i replay v1;
- brak zmian selector score, IWIM, sender, Jito, execution ani post-buy;
- authority Profile A i aktywny dev source pozostają bez zmian;
- checked-in Ghost Brain TOML pozostaje bez zmian;
- rollout pozostaje `Legacy`;
- PR2C jest OFF w live oraz OFF by default w shadow;
- nie aktywowano DualCompute/V2;
- nie rozpoczęto PR3 ani Type-5 T1.

## 11. Znane baseline warnings/failures

Targeted Clippy raportuje liczne istniejące warnings poza zmienionymi liniami.
Nie są one przypisane finalnemu amendmentowi. Znany historyczny
`selector_shadow_score_filters_non_finite_feature_values` pozostaje baseline
spoza PR2C; selector score i jego owner nie zostały zmienione.

## 12. Status końcowy

Lokalna implementacja i pełna walidacja finalnej dyrektywy przeszły. Do
publikacji pozostają commit, push, jedno CI i końcowy review zdalnego heada.
PR nadal pozostaje draftem, a prospective burn-in nie jest autoryzowany.

Markery finalnej dyrektywy:

```text
DURABLE_EVIDENCE_READY
REPLAY_AND_COMPARATOR_READY
RUNTIME_ISOLATION_PASS
LEGACY_AND_LIVE_BEHAVIOR_UNCHANGED
PROSPECTIVE_BURN_IN_NOT_AUTHORIZED
PR3_NOT_STARTED
```
