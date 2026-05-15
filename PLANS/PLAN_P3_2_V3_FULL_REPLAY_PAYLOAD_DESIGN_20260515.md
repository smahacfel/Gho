# Plan P3.2 V3 Full Replay Payload Design - 2026-05-15

## Decyzja

P3.2 ma przeprowadzic Ghost V3 z `hash_only` do audytowalnego full replay i
counterfactual readiness, bez P2 promotion.

Rekomendacja projektowa:

- nie probowac rekonstruowac pelnego replay z obecnych rozproszonych pol JSONL,
- dodac addytywny, wersjonowany payload materialized snapshot do V3 shadow rows,
- dodac row-level policy config payload albo fail-closed, gdy aktualny config hash
  nie pasuje do row,
- zbudowac validator Rust-first, ktory deserializuje snapshot, przelicza hash,
  uruchamia V3 evaluator i dopiero wtedy pozwala na counterfactual ablation.

To jest projekt P3.2. Nie implementuje P2, nie zmienia scoringu i nie dotyka
active V2/V2.5/IWIM/execution.

## Aktualny Stan

P1/P3 evidence:

- P1 baseline: `status=ok`, `v3_rows=86`, `replay_status=hash_only`.
- P3 gate: `p3_status=insufficient_data`, `promotion_ready_gates=[]`.
- P3.1: `REJECT_V3_MANIPULATION_CONTRADICTION` ma realny hard-risk signal, ale
  pozostaje zablokowany przez `hash_only` i degraded evidence.

Aktualne skrypty:

- `scripts/v3_shadow_report.py` juz rozpoznaje potencjalne pola full replay:
  - `v3_feature_snapshot`
  - `v3_feature_snapshot_payload`
  - `v3_materialized_feature_snapshot`
- W obecnych rows tych payloadow nie ma, wiec raport uczciwie zwraca
  `hash_only`.

Aktualny runtime/logger:

- `ghost-launcher/src/oracle_runtime.rs` loguje:
  - `v3_policy_config_hash`
  - `v3_feature_snapshot_hash`
  - `v3_materialization_version`
  - `v3_policy_version`
  - `v3_stage_thresholds`
  - `v3_component_scores`
  - `v3_actionability`
  - wybrane V3 feature payloads: evidence, organic broadening,
    manipulation contradictions.
- `ghost-brain/src/oracle/decision_logger.rs` ma addytywne `Option<T>` pola V3
  z `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- `ghost-launcher/src/components/gatekeeper_v3.rs::v3_feature_snapshot_hash()`
  juz buduje kanoniczny payload z V3-relewantnych pol `MaterializedFeatureSet`,
  ale payload jest lokalny w funkcji i nie jest logowany.

Rozmiar obecnego P1 baseline:

- `86` primary `v25_shadow` rows,
- JSONL total: okolo `1.78 MB`,
- sredni row: okolo `20.7 KB`,
- min/max row: okolo `19.8 KB` / `21.5 KB`.

## Cel P3.2

Umozliwic:

- full replay parity dla V3 shadow rows,
- recompute `evaluate_v3_from_features()` z logu,
- prawdziwa counterfactual ablation:
  - `no manipulation_contradiction`,
  - `no organic_broadening`,
  - `no sybil/FSC/CPV caps`,
  - `no alpha cap`,
  - `no execution cap`,
- fail-closed certification zamiast proxy po reason group.

Nie jest celem:

- P2 promotion,
- zmiana active policy,
- tuning progow,
- runtime use of outcome labels,
- zastapienie `MaterializedFeatureSet` innym SSOT.

## Decyzja: Full Snapshot vs Rekonstrukcja

### Opcja A - deterministyczna rekonstrukcja z obecnych pol JSONL

Odrzucona dla P3.2.

Powody:

- obecne rows nie zawieraja pelnego `MaterializedFeatureSet`,
- `v3_shadow_*` pola sa diagnostyka wyniku, nie pelny input evaluator,
- event-dir/lifecycle merge nie jest jeszcze zaimplementowany i musi
  fail-closed,
- rekonstrukcja z rozproszonych pol wymagalaby ukrytych zalozen o brakujacych
  domenach evidence.

### Opcja B - logowac tylko kanoniczny hash payload

Mozliwa, ale nie rekomendowana jako pierwszy full replay payload.

Plus:

- jest deterministyczny,
- jest blisko obecnego `v3_feature_snapshot_hash()`,
- floaty sa reprezentowane przez bit patterns.

Minus:

- payload nie jest naturalnym `MaterializedFeatureSet`,
- wymaga osobnego adaptera bit-json -> Rust types,
- latwo stworzyc drugi quasi-SSOT obok `MaterializedFeatureSet`,
- utrudnia szybkie uzycie istniejacego `evaluate_v3_from_features()`.

### Opcja C - logowac typed materialized snapshot payload

Rekomendowana dla P3.2 V1.

Dodac pole:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub v3_materialized_feature_snapshot: Option<serde_json::Value>,
```

Payload powinien byc `serde_json::to_value(&assessment.feature_snapshot)` z
wersja payloadu i materialization version logowanymi obok. Validator deserializuje
payload z powrotem do `MaterializedFeatureSet`, przelicza obecny kanoniczny
`v3_feature_snapshot_hash(features, materialization_version)` i porownuje z
logowanym `v3_feature_snapshot_hash`.

Uzasadnienie:

- zachowuje `MaterializedFeatureSet` jako SSOT,
- nie wymaga nowego evaluator input modelu,
- minimalizuje ryzyko rozjazdu replay path z runtime path,
- stare JSONL bez pola dalej parsuja sie jako `hash_only`,
- payload moze byc pozniej optymalizowany, ale najpierw trzeba uzyskac poprawny
  full replay.

## Minimalny Schemat P3.2

Nowe addytywne pola JSONL:

```text
v3_replay_payload_schema_version: Option<u32>   # start: 1
v3_materialized_feature_snapshot: Option<Value> # serde MaterializedFeatureSet
v3_policy_config_payload: Option<Value>         # canonical V3 policy payload
```

Istniejace pola pozostaja:

```text
v3_policy_config_hash
v3_feature_snapshot_hash
v3_materialization_version
v3_policy_version
v3_stage_thresholds
v3_component_scores
v3_actionability
```

Zasady:

- Wszystkie nowe pola maja `#[serde(default, skip_serializing_if =
  "Option::is_none")]`.
- Nie kasowac ani nie zmieniac znaczenia obecnych P1/P3 fields.
- `log_schema_version` nie musi byc podbijany, jesli repo utrzymuje addytywne
  V3 sidecar fields przez `Option<T>`, ale `v3_replay_payload_schema_version`
  jest obowiazkowy dla nowych rows z payloadem.
- `v3_policy_config_payload` powinien byc kanonicznym payloadem uzywanym do
  `v3_policy_config_hash()`. Jesli nie zostanie dodany, validator moze uzyc
  aktualnego configu tylko wtedy, gdy jego hash rowna sie
  `v3_policy_config_hash`; w przeciwnym razie ma fail-closed jako
  `policy_config_unavailable`.

## Gating w Configu

Dodac w `GatekeeperV3Config`:

```rust
#[serde(default)]
pub replay_payload_enabled: bool,
```

Default:

- `false` w struct defaultach i dla starych TOML,
- `true` tylko w shadow-burnin P3.2 profile, jesli chcemy zebrac nowe full replay
  evidence.

Warunek emisji:

```text
shadow_emit_enabled == true
AND replay_payload_enabled == true
AND decision_plane == v25_shadow / V3 sidecar terminal row
```

Nie emitowac payloadu dla legacy live plane ani aktywnej sciezki wykonania.

## Validator Fail-Closed

Nowy validator powinien byc Rust-first, zeby uzyc dokladnie tych samych typow i
funkcji co runtime:

```text
scripts/v3_full_replay_report.py       # wrapper/report
ghost-launcher/src/bin/v3_replay.rs    # albo test/helper Rust
```

Minimalne kroki dla kazdego row:

1. Wczytaj JSONL.
2. Wybierz rows z V3 fields.
3. Jesli nie ma `v3_materialized_feature_snapshot`, row jest `hash_only`.
4. Jesli payload jest obecny:
   - wymagaj `v3_replay_payload_schema_version`,
   - deserializuj payload do `MaterializedFeatureSet`,
   - wymagaj `v3_materialization_version`,
   - przelicz `v3_feature_snapshot_hash(features, materialization_version)`,
   - porownaj z row `v3_feature_snapshot_hash`,
   - odtworz `GatekeeperV3Config` z `v3_policy_config_payload` albo aktualnego
     TOML, tylko gdy hash pasuje,
   - uruchom `evaluate_v3_from_features(&features, &config, deadline_elapsed)`,
   - porownaj verdict, reason, risk penalty, opportunity score i confidence z
     row.
5. Jesli cokolwiek nie pasuje, validator zwraca fail-closed, nie partial OK.

Statusy:

```text
full_replay_ok
hash_only
payload_deserialize_failed
snapshot_hash_mismatch
policy_config_hash_mismatch
policy_config_unavailable
verdict_mismatch
reason_mismatch
score_mismatch
duplicate_ab_record_conflict
```

`replay_status="full"` tylko gdy wszystkie V3 rows z payloadem przejda parity.

## Counterfactual Ablation Design

Dopiero po `full_replay_ok` wolno liczyc prawdziwe ablation.

Minimalne warianty:

- full V3 baseline,
- no `manipulation_contradiction`,
- no `organic_broadening`,
- no sybil/FSC/CPV caps,
- no alpha cap,
- no execution cap.

Implementacyjnie:

- Nie zmieniac TOML runtime.
- Nie zmieniac active policy.
- Ablation robi kopie `GatekeeperV3Config` albo kopie `MaterializedFeatureSet`
  w procesie offline.
- Output musi oddzielac:
  - changed verdict,
  - changed reason,
  - changed score only,
  - no change.

Dla P3.1 najwazniejszy wariant:

```text
no manipulation_contradiction
```

Ten wariant ma odpowiedziec, czy `70/86` zmienia sie z `REJECT` na `PENDING`,
czy tylko zmienia reason/cap przy nadal zerowej confidence z innych degradacji.

## Rozmiar JSONL i Koszt

Obecny primary P1 row ma srednio okolo `20.7 KB`. Full payload zwiekszy row size.
P3.2 powinien wprowadzic budzet:

```text
warn_avg_row_bytes_over=64KB
fail_avg_row_bytes_over=128KB
warn_total_decision_log_over=250MB
```

Zasady kosztowe:

- Payload tylko w shadow V3 terminal rows.
- Payload gated configiem.
- Nie logowac payloadu dla kazdej intermediate runtime obserwacji.
- Nie duplikowac payloadu w legacy plane.
- Raport ma pokazywac:
  - avg row bytes,
  - p95 row bytes,
  - max row bytes,
  - total JSONL bytes,
  - payload_present rows.

Opcjonalna optymalizacja po V1:

- NDJSON gzip artifact dla payload-heavy runow,
- compact canonical payload jako V2, dopiero po udowodnieniu parity z typed
  snapshot payloadem.

## Backward Compatibility

Stare TOML:

- `replay_payload_enabled=false` przez default.

Stare JSONL:

- Brak payloadu oznacza `hash_only`.
- `scripts/v3_shadow_report.py` i `scripts/v3_replay_ablation_report.py` nie
  moga failowac starych rows tylko dlatego, ze nie maja P3.2 payloadu.

Nowe JSONL:

- Payload optional i addytywny.
- Validator moze mieszac stare i nowe rows, ale certification musi rozdzielac:
  - `full_replay_rows`,
  - `hash_only_rows`,
  - `failed_replay_rows`.

## Bezpieczenstwo Kontraktow

P3.2 nie moze naruszyc:

- `MaterializedFeatureSet` jako SSOT,
- active V2/V2.5 policy,
- `GatekeeperBuffer::evaluate_from_features()`,
- IWIM,
- live sender / execution,
- typed reason code ownership,
- shadow/live separation.

Payload ma sluzyc tylko replay/audit/counterfactual offline.

## Plan Implementacyjny po Akceptacji

### Commit 1 - schema/config only

- Dodac `replay_payload_enabled=false` do `GatekeeperV3Config`.
- Dodac optional fields do `GatekeeperBuyLog`:
  - `v3_replay_payload_schema_version`,
  - `v3_materialized_feature_snapshot`,
  - `v3_policy_config_payload`.
- Testy:
  - stare TOML bez pola laduja sie,
  - stare JSONL bez payloadu parsuja sie,
  - nowe JSONL serializuje pola tylko gdy Some.

### Commit 2 - runtime emission gated

- W `enrich_gatekeeper_log_with_v3_shadow()` logowac payload tylko gdy:
  - `shadow_emit_enabled=true`,
  - `replay_payload_enabled=true`.
- Nie emitowac payloadu, gdy `shadow_emit_enabled=false`.
- Testy runtime/logging:
  - payload absent przy default false,
  - payload present przy true,
  - `v3_feature_snapshot_hash` recompute z payloadu rowna sie row hash.

### Commit 3 - replay validator

- Dodac Rust-backed replay validator.
- `scripts/v3_replay_ablation_report.py` uzywa validator output albo przynajmniej
  rozpoznaje `full` vs `hash_only`.
- Fail-closed dla:
  - deserialize errors,
  - hash mismatch,
  - config mismatch,
  - verdict mismatch.

### Commit 4 - true ablation

- Po full replay parity dodac counterfactual variants.
- P3.1 ma przejsc z reason-group proxy na realny `no manipulation_contradiction`
  recompute.

### Commit 5 - clean P3.2 evidence run

- Nowy rerun w tym samym P1/P3 namespace albo nowym jawnie nazwanym P3.2
  namespace, bez mieszania z P0.
- Raport:
  - `replay_status=full`,
  - `full_replay_rows > 0`,
  - `hash_mismatch=0`,
  - `verdict_mismatch=0`,
  - row size budget OK.

## Acceptance P3.2

P3.2 mozna uznac za domkniete dopiero gdy:

- stare JSONL bez payloadu nadal raportuja `hash_only`, nie fail,
- nowe JSONL z payloadem przechodza `full_replay_ok`,
- validator recomputuje `v3_feature_snapshot_hash` z payloadu,
- validator potrafi odtworzyc V3 verdict/reason/score,
- policy config jest dopasowany po hash albo fail-closed,
- true ablation dla `no manipulation_contradiction` nie jest juz proxy,
- P2 promotion pozostaje `false` / nieuruchomione.

## Werdykt Projektowy

Recommended path:

```text
log typed MaterializedFeatureSet replay payload
log or validate matching V3 policy config payload
build Rust-first fail-closed replay validator
only then enable true counterfactual ablation
```

Nie rekomenduje P2 przed P3.2/P3.3. `REJECT_V3_MANIPULATION_CONTRADICTION`
pozostaje kandydatem do przyszlego ADR, ale tylko po full replay i multi-run
stability evidence.
