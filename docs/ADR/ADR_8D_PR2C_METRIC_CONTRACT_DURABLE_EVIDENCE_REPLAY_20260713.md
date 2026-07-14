# ADR-8D: PR2C durable metric-contract evidence, replay v2 i audit

Status: `AMENDED / IMPLEMENTED / PASS / READY FOR RE-REVIEW`

Typ: ADR-8D / durability, replay, comparator, audit i rollout safety

Data: 2026-07-13

Repo: `smahacfel/Gho`

Branch: `agent/metric-contract-pr2c-durable-evidence-replay`

Base i merge-base: `fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9`

Plan normatywny:
`PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`

Poprzedni etap:
`docs/ADR/ADR_8D_PR2B_METRIC_CONTRACT_EVIDENCE_ONLY_PRODUCERS_20260712.md`

Raport dowodowy:
`reports/metric_contracts/pr2c_durable_evidence_replay_verification_v1.md`

Amendment po blocking review:
`docs/ADR/ADR_8D_PR2C_REVIEW_BLOCKERS_DURABILITY_AUDIT_20260713.md`

Amendment po drugim review:
`docs/ADR/ADR_8D_PR2C_SECOND_REVIEW_DURABLE_EQUIVALENCE_RESOURCE_INTEGRITY_20260713.md`

Poziom ryzyka: `HIGH`. Zmiana przecina terminalną materializację, DecisionLogger,
durable JSONL, replay i narzędzia audytowe. Ryzyko wpływu na decyzje jest
ograniczone przez rollout `Legacy`, niezmienione authority Profile A, osobny
addytywny v34 stream oraz zerowy wpływ comparatora na terminalny verdict.

## 1. Kontekst

PR2B materializuje dziesięć rodzin raz i przechowuje wyłącznie compact Wire V1
projection w `MaterializedFeatureSet`. Pełny evidence set był dotąd wyłącznie
in-memory. Bez trwałego, sparowanego rekordu nie można było później udowodnić,
że decision-time projection pochodziła z dokładnie tego full evidence, ani
wykonać fail-closed replay i policy-drift audit.

PR2C domyka ten przepływ bez uruchamiania producentów ponownie:

```text
one frozen producer input set
→ Pr2bCompleteMetricContractSnapshotV1
  ├─ full MetricContractsEvidenceSetV1
  └─ compact MetricContractDecisionEvidenceProjectionV1
→ MFS Wire V1 projection
→ compact v34 summary + paired full evidence sidecar
→ replay v2 + comparator + single-run/bundle audit
```

## 2. Zamrożony codebook Wire V1

`MetricContractProjectionWireV1SchemaManifest::current()` publikuje pełny,
uporządkowany codebook: wire version, 18 tuple-layout tables i 28 enum/reason
mapping tables. Kolejność tabel, ich nazwy, pozycje i kody są semantyczne.

Machine-readable golden:
`reports/metric_contracts/metric_contract_wire_v1_schema_manifest.json`

Frozen BLAKE3:

```text
70d79931f3f9a82720e46f622d439930a087431e305d14c02d88dcd26568fc7f
```

Replay v2 sprawdza bieżący codebook względem tego hasha przed odbudową
projection. Golden pojedynczego payloadu PR2B pozostaje dodatkowym testem, ale
nie zastępuje pełnego manifestu. Zmiana Wire V1 wymaga Wire V2.

Canonical projection hash pozostaje SHA-256/JCS domain semantic payloadu.
Wire bytes nie są hashowaną tożsamością semantyczną.

## 3. Jeden snapshot i dwie durable reprezentacje

Terminalna materializacja przechowuje poza MFS dokładnie wynik tego samego
wywołania PR2B, którego projection została atomowo przypisana do MFS. Terminalny
logger konsumuje snapshot raz. Nie czyta ponownie transakcji, indeksów, zegara
producenta ani live owner state; nie rekonstruuje full evidence z projection.

Runtime-only monotonic start jest przechowywany obok, a nie wewnątrz semantic
snapshotu. Powstaje bezpośrednio przed pierwszym canonical producer call i jest
przenoszony aż do writer-owned final bytes. Dzięki temu przerwy między
materializacją, comparatorem i writerem nie znikają z pomiaru, a losowe czasy
wykonania nadal nie wpływają na equality, serde ani semantic hash.

Stable underlying-event identity pochodzi wyłącznie z source signature
pool-creation transaction, gdy ingest ją dostarcza. Nie jest wyprowadzana z
`join_key`. Brak signature pozostaje `Null/NOT_EVALUABLE` dla collision gate.

## 4. Compact v34 i pełny sidecar

v34 ma dokładnie zatwierdzone piętnaście pól. Nie zawiera MFS projection, full
evidence, owner/event collections ani Type-5. `GatekeeperBuyLog` nadal emituje
i parsuje schema v33 bez nowych wymaganych pól.

Full sidecar `metric_contract_evidence_v1.jsonl` używa istniejących:

- `MetricContractEvidenceHashPayloadV1`;
- `MetricContractEvidenceTransportV1`;
- `MetricEvidenceRecordIdentityV1`;
- `StableEventIdentityV1`.

`evidence_sha256` jest SHA-256 canonical semantic payloadu bez writer timestamp
i rotation part, ale z niezależnym durable `source_cutoff`. Konstrukcja oraz
deserializacja wykonują profile, envelope, family semantic i hash validation.
Payload utrwala również authoritative/comparator normalized policy snapshots,
comparator evaluability, policy version i Gatekeeper config hash. Replay
recomputuje z nich exact `equivalence_deltas` zamiast ufać v34.
Exact effective-config payload jest
utrwalony w rotation manifest i musi odpowiadać hashom v34/evidence w replayu.

## 5. Jeden bounded paired writer command

v34 summary i evidence transport wchodzą jako jeden wariant istniejącej,
bounded kolejki DecisionLogger. Zapis dwóch plików nie jest nazywany
filesystem-atomic. Writer utrzymuje osobne liczniki commandów, obu rows,
summary/evidence failures, disable, send failure, drop, orphan, missing pair,
enqueue wait i queue high-water.

ENOSPC wyłącza writer fail-closed. Evidence failure po summary tworzy jawny
`orphan_summary`; odwrócona fault-injection regression dowodzi również
`orphan_evidence`. Channel close i disabled logger zwracają typed enqueue
error. Mid-row short write zapisuje rzeczywisty prefiks i pozostaje wykrywalny
jako truncated part. Session/MFS/producer locks są zwalniane przed queue await
i przed filesystem I/O.

Part 0 zachowuje nazwy:

```text
metric_contract_decisions_v34.jsonl
metric_contract_evidence_v1.jsonl
```

Kolejne parts mają wspólny pięciocyfrowy indeks. Manifest zapisuje relative
path, schema, part index, rows, bytes, first/last full record identity, SHA-256
całego partu, run/build/Gatekeeper/brain/profile/effective-config provenance,
Wire codebook hash i bounded resource histograms. Part data
jest `sync_data()`, manifest jest `sync_all()` przed rename, a katalog po rename
jest `sync_all()`. `writer_finalized=true` jest ustawiane dopiero po poprawnej
finalizacji; audit nie uznaje nadal mutable manifestu za immutable run. Audit
odrzuca brakujący, dodatkowy, zmieniony, ucięty, nieciągły lub niesparowany
part oraz path wychodzący poza run directory.

Part 0 zamraża run provenance; każdy kolejny summary/evidence part musi być mu
exact równy. Histogramy wymagają frozen bucket codebooku, checked sumy bucketów,
jednej próbki na paired command oraz spójnego `max_us`/overflow.

## 6. Record identity i collision contract

Duplicate record identity oznacza wyłącznie identyczne:

```text
(run_id, join_key, decision_plane)
```

Ten sam `join_key` w innym runie nie jest duplicate. Cross-run underlying-event
collision jest osobnym gate’em używającym `StableEventIdentityV1`. Brak stable
identity nigdy nie daje clean zero; single-run/bundle otrzymuje
`NOT_EVALUABLE`, chyba że przyszły osobny frozen partition proof formalnie
udowodni rozłączność. PR2C nie dodaje takiego fallbacku.

## 7. Comparator

Equivalence comparator wykonuje rzeczywistą drugą, czystą ewaluację istniejącej
Gatekeeper policy na tym samym frozen `GatekeeperAssessment` i dokładnym
`GatekeeperV2Config`. Nie czyta live state, nie uruchamia IWIM ani execution,
nie emituje terminal eventu i nie zmienia authority.

Normalizer porównuje exact:

- verdict;
- typed primary reason code;
- ordered reason-chain identity;
- sześciopolowy phase-pass vector;
- soft points;
- selector soft score;
- hard-fail classification.

Dowolny drift jest poprawnym, trwałym wynikiem komparatora: v34 i sidecar są
zapisywane, replay przechodzi, a audit zwraca `FAIL_POLICY_DRIFT`. Brak
porównywalnej authoritative decyzji daje `NotEvaluable`, nie fikcyjne `Equal`.
Replay wymaga exact równości v34 deltas z deltami ponownie wyliczonymi z
zahashowanych durable policy snapshots.
Semantic counterfactual lane obserwuje wyłącznie dev-primary i corrected FTDI
actionability. Dwie obecne, różne wartości emitują typed
`COUNTERFACTUAL_POLICY_DELTA_OBSERVED:<lane>:...`; brak którejkolwiek wartości
jest `NotEvaluable`. Delta nie zmienia terminalnego verdictu ani authority.

## 8. Replay v1 i replay v2

Frozen `v3_replay.rs` pozostaje schema v1 i nie importuje nowych evidence types.
Historyczny MFS bez projection nadal deserializuje pole do `None` bez measured
reconstruction.

Replay v2:

1. weryfikuje part manifest i SHA w warstwie audytu;
2. weryfikuje full evidence transport semantic hash;
3. łączy v34, sidecar i v33 decision-time MFS po pełnej identity/hash;
4. sprawdza profile i exact effective-config payload/hash;
5. buduje frozen context z niezależnego, zahashowanego durable cutoffu i
   odbudowuje projection wyłącznie z full evidence;
6. wymaga exact domain equality z decision-time MFS projection;
7. wymaga identycznego canonical semantic projection hash;
8. wykonuje Wire V1 round-trip po sprawdzeniu codebook manifestu;
9. recomputuje equivalence deltas z durable policy snapshots;
10. recomputuje v34 contract sets, measured mask i counterfactual semantics;
11. odrzuca unknown/partial schema i każdy context/cutoff/summary drift.

Transportowe timestampy i rotation index nie uczestniczą w domain equality.

## 9. Audit CLI

Jeden binary `metric-contract-audit` ma tryby `single-run` i `bundle` oraz
terminalne klasy:

```text
PASS_CUTOVER_READY
NOT_EVALUABLE
FAIL_SCHEMA_OR_REPLAY
FAIL_POLICY_DRIFT
FAIL_RESOURCE_BUDGET
```

Per-run replay/resource PASS następuje przed agregacją bundle. Single-run
wymaga exact bijekcji current v33 ↔ v34 ↔ evidence. Bundle sprawdza unikalne
run IDs, globalne full identities, build cleanliness, Gatekeeper config,
rollout, profile, schemas, Wire hash, effective-config, non-overlap, UTC
buckets wszystkich paired cutoffów oraz stable-event collisions.
`brain_config_hash` jest frozen w obrębie
jednego runu, lecz provenance-only pomiędzy runami. Każdy raport zawiera typed
`cutover_scope = metric_contracts_v1_1_profile_a_equivalence_only`.

## 10. Status BURN contractu

Finalizacyjny amendment z 2026-07-14 wycofuje `BURN_IN_CONTRACT_V2` oraz jego
publiczny audit entry point przed zebraniem jakiegokolwiek prospective row.
V1 i V2 są historycznymi pre-run draftami, nie aktywnymi kontraktami. PR2C nie
autoryzuje prospective burn-in; przyszły contract wymaga osobnej decyzji
właściciela i nie może dziedziczyć rows z tych draftów.

Historyczne v33 pozostaje feasibility-only: nie ma v34, full evidence,
effective-config hash, stable identity ani rotation manifestu, więc replay v2
odrzuca je zamiast rekonstruować V2 evidence z legacy scalarów.

## 11. Resource contract po finalizacji

Normatywne rozmiary są liczone na nieskompresowanym JSON: Wire V1, v34 i full
evidence transport. Latency całej durable ścieżki jest raportowana
diagnostycznie jako p50/p95/p99/max, bez merge lub burn-in threshold. Audit
sprawdza jednocześnie:

- osobną bounded PR2C queue i nieblokujący `try_send`;
- queue high-water `< 80%` dla prawidłowego evidence runu;
- zero drops, send failures, writer failures, orphanów i truncated rows;
- v34 p95 delta `<= 8 KiB` i `<= 10%` względem paired v33;
- Wire V1 p95 `<= 12 KiB`, max `<= 16 KiB`;
- sidecar p95 `<= 24 KiB`, p99 `<= 48 KiB`;
- combined byte-rate delta `<= 25%` względem paired v33.

`metric_contract_build_and_serialize_us`, projection, comparator, enqueue oraz
serialization histograms pozostają integralnymi diagnostykami. Nie mogą
nadawać runowi prospective/cutover authority. Własny JCS serializer i
fixed-width JSON telemetry substitution usunięto; canonical hashing ponownie
używa referencyjnego `serde_json_canonicalizer`.

## 12. Zakres wyłączony

Bez zmian pozostają active Gatekeeper V2/V2.5/V3 behavior, thresholds, weights,
phases, soft points, verdicts, primary reasons, authority Profile A, IWIM,
execution, sender, Jito, post-buy, selector score, active dev source i
live/shadow boundary. Rollout pozostaje `Legacy`.

PR2C nie implementuje Type-5, v35, PR3 cutover, policy promotion, DualCompute/V2
activation ani nowych progów Gatekeepera.

## 13. Konsekwencje

Pozytywne:

- evidence/projection/v34/replay mają jedną immutable source identity;
- corruption, truncation, mismatch i policy drift są fail-closed i audytowalne;
- v33 i V3 replay v1 zachowują historyczną kompatybilność;
- prospective burn-in pozostaje jawnie nieautoryzowany.

Koszt:

- dwa dodatkowe bounded JSONL streams i manifest;
- druga czysta ewaluacja policy do equivalence proof;
- terminalny run jest nie-evaluable przy braku stable identity lub kompletnego
  provenance zamiast otrzymywać optymistyczny PASS.

## 14. Decyzja końcowa

PR2C pozostaje domyślnie OFF i może być świadomie włączony wyłącznie dla
dedykowanych shadow evidence runs. Merge nie autoryzuje prospective burn-in;
bieżący rollout pozostaje `Legacy`.

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
GATEKEEPER_POLICY_UNCHANGED
V3_V1_REPLAY_UNCHANGED
TYPE5_NOT_STARTED
DURABLE_EVIDENCE_READY
REPLAY_AND_COMPARATOR_READY
RUNTIME_ISOLATION_PASS
LEGACY_AND_LIVE_BEHAVIOR_UNCHANGED
PROSPECTIVE_BURN_IN_NOT_AUTHORIZED
PR3_NOT_STARTED
```
