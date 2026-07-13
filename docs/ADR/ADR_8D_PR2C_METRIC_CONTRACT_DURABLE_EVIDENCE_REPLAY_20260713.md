# ADR-8D: PR2C durable metric-contract evidence, replay v2 i audit

Status: `ACCEPTED / IMPLEMENTED / LOCALLY VERIFIED`

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

Runtime-only timing jest przechowywany obok, a nie wewnątrz snapshotu. Dzięki
temu losowe czasy wykonania nie wpływają na equality, serde ani semantic hash.

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
i rotation part. Konstrukcja oraz deserializacja wykonują profile, envelope,
family semantic i hash validation. Exact effective-config payload jest
utrwalony w rotation manifest i musi odpowiadać hashom v34/evidence w replayu.

## 5. Jeden bounded paired writer command

v34 summary i evidence transport wchodzą jako jeden wariant istniejącej,
bounded kolejki DecisionLogger. Zapis dwóch plików nie jest nazywany
filesystem-atomic. Writer utrzymuje osobne liczniki commandów, obu rows,
summary/evidence failures, disable, send failure, drop, orphan, missing pair,
enqueue wait i queue high-water.

ENOSPC wyłącza writer fail-closed. Summary failure nie uruchamia evidence write.
Evidence failure po summary tworzy jawny `orphan_summary`. Channel close i
disabled logger zwracają typed enqueue error. Session/MFS/producer locks są
zwalniane przed queue await i przed filesystem I/O.

Part 0 zachowuje nazwy:

```text
metric_contract_decisions_v34.jsonl
metric_contract_evidence_v1.jsonl
```

Kolejne parts mają wspólny pięciocyfrowy indeks. Manifest zapisuje relative
path, schema, part index, rows, bytes, first/last full record identity, SHA-256
całego partu, run/build/Gatekeeper/profile/effective-config provenance i bounded
resource histograms. `writer_finalized=true` jest ustawiane dopiero po
zamknięciu writer task; audit nie uznaje nadal mutable manifestu za immutable
run. Audit odrzuca brakujący, dodatkowy, zmieniony, ucięty, nieciągły lub
niesparowany part.

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

Dowolny drift blokuje zbudowanie pair i daje `FAIL_POLICY_DRIFT` w audycie.
Semantic counterfactual lane obserwuje wyłącznie dev-primary i corrected FTDI
actionability. Delta ustawia diagnostyczne `counterfactual_delta_present`, ale
nie zmienia terminalnego verdictu ani authority.

## 8. Replay v1 i replay v2

Frozen `v3_replay.rs` pozostaje schema v1 i nie importuje nowych evidence types.
Historyczny MFS bez projection nadal deserializuje pole do `None` bez measured
reconstruction.

Replay v2:

1. weryfikuje part manifest i SHA w warstwie audytu;
2. weryfikuje full evidence transport semantic hash;
3. łączy v34, sidecar i v33 decision-time MFS po pełnej identity/hash;
4. sprawdza profile i exact effective-config payload/hash;
5. odbudowuje projection wyłącznie z full evidence i frozen context;
6. wymaga exact domain equality z decision-time MFS projection;
7. wymaga identycznego canonical semantic projection hash;
8. wykonuje Wire V1 round-trip po sprawdzeniu codebook manifestu;
9. odrzuca unknown/partial schema i context/cutoff drift.

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

Per-run replay/resource PASS następuje przed agregacją bundle. Bundle sprawdza
build, Gatekeeper config, profile, schemas, effective-config, non-overlap, UTC
buckets, full identity duplicates, stable-event collisions oraz minima frozen
burn-in contractu.

## 10. BURN_IN_CONTRACT_V1

Machine-readable frozen contract:
`reports/metric_contracts/BURN_IN_CONTRACT_V1.json`

Canonical hash:

```text
56ceb5a80a0b6d413cf639f0ac02d30fade2770f7f5c4cf4a1a014f3632ae7df
```

Contract utrwala minimum 3 niepokrywających się runów, 1 h per run, dwa UTC
4-hour buckets, 8 h aggregate, 700 decisions, 100 dev-known, 100 clean Flip V2
evaluable i 30 real dev legacy/V2 divergences oraz wszystkie resource limits.
Rows z timestampem niepóźniejszym niż `frozen_at` nie wchodzą do prospective
counts. Zmiana gate’u wymaga nowej wersji/hash/frozen_at i nowych rows.

Historyczne v33 pozostaje feasibility-only: nie ma v34, full evidence,
effective-config hash, stable identity ani rotation manifestu, więc replay v2
odrzuca je zamiast rekonstruować V2 evidence z legacy scalarów.

## 11. Resource contract

Normatywne rozmiary są liczone na nieskompresowanym JSON: Wire V1, v34 i full
evidence transport. Bounded histogramy w run manifest utrwalają p99 dla
projection build/validation, full build+serialization i enqueue wait. Audit
sprawdza jednocześnie:

- comparator, full build+serialize, projection build/validate i enqueue p99
  `<= 1_000 us`;
- queue high-water `< 80%`;
- zero drops, failures i orphans;
- v34 p95 delta `<= 8 KiB` i `<= 10%` względem paired v33;
- Wire V1 p95 `<= 12 KiB`, max `<= 16 KiB`;
- sidecar p95 `<= 24 KiB`, p99 `<= 48 KiB`;
- combined byte-rate delta `<= 25%` względem paired v33.

Release measurements oraz dokładne komendy są utrwalone w raporcie
weryfikacyjnym, nie jako surowe benchmark logs. Pełny release path uzyskał:

```text
metric_contract_build_and_serialize_us p99 = 796
metric_contract_projection_build_and_validate_us p99 = 919
comparator_elapsed_us p99 = 10
logger_enqueue_wait_us p99 upper bound = 32
writer_queue_high_water = 12.8%
Wire V1 p95/max = 2339/2339 bytes
sidecar p95/p99 = 21406/21406 bytes
v34 p95 = 1167 bytes
```

Wszystkie zamrożone limity czasu, rozmiaru, kolejki i combined storage
przechodzą jednocześnie.

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
- prospective burn-in ma zamrożone, machine-readable gates.

Koszt:

- dwa dodatkowe bounded JSONL streams i manifest;
- druga czysta ewaluacja policy do equivalence proof;
- terminalny run jest nie-evaluable przy braku stable identity lub kompletnego
  provenance zamiast otrzymywać optymistyczny PASS.

## 14. Decyzja końcowa

Pełna macierz PR2A/PR2B/PR2C, Gatekeeper i replay przeszła. Release resource
harness, paired-writer fault matrix, replay equality, comparator drift matrix,
single-run/bundle audit oraz forbidden-scope proof spełniają kontrakt PR2C.

PR2C jest gotowy do draft review i prospective burn-in pod zamrożonym
`BURN_IN_CONTRACT_V1`. Nie jest to zgoda na policy promotion, PR3, Type-5,
DualCompute ani V2 rollout. Rollout pozostaje `Legacy`.
