# ADR-8D: PR2C — domknięcie blockerów durability, replay i audit

Status: `AMENDED BY SECOND AND THIRD REVIEW / HISTORICAL IMPLEMENTATION RECORD`

Typ: ADR-8D / review amendment / durable evidence / replay / rollout safety

Data: 2026-07-13

Repo: `smahacfel/Gho`

Branch: `agent/metric-contract-pr2c-durable-evidence-replay`

Base i merge-base: `fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9`

Amendowany ADR:
`docs/ADR/ADR_8D_PR2C_METRIC_CONTRACT_DURABLE_EVIDENCE_REPLAY_20260713.md`

Raport dowodowy:
`reports/metric_contracts/pr2c_durable_evidence_replay_verification_v1.md`

Nowszy amendment:
`docs/ADR/ADR_8D_PR2C_SECOND_REVIEW_DURABLE_EQUIVALENCE_RESOURCE_INTEGRITY_20260713.md`

Bieżący amendment:
`docs/ADR/ADR_8D_PR2C_RUNTIME_ROUTING_PRODUCTION_RESOURCE_P99_20260713.md`

Poziom ryzyka: `HIGH`. Zmiana dotyka terminalnego evidence recordu, paired
writera, durable provenance, replayu oraz klasyfikacji single-run/bundle audit.
Nie zmienia aktywnej Gatekeeper policy, authority Profile A ani rollout mode
`Legacy`.

## 1. Problem

Review PR #65 wykazało siedem blockerów oraz osiem problemów major. Najbardziej
niebezpieczne kontrprzykłady umożliwiały:

- zaniżenie addytywnego kosztu storage;
- deklarowanie resource PASS bez pomiaru producer→final-bytes;
- utratę v34/evidence przy policy drift;
- pominięcie current v33 bez paira;
- kołową walidację decision cutoff;
- fałszywe minima bundle/BURN;
- zmianę semantic fields v34 bez wykrycia w replayu.

Pierwotne markery readiness zostały wycofane z opisu draft PR na czas
amendmentu. PR pozostaje draftem.

## 2. Decyzje kontraktowe

### 2.1 Exact trzystronna bijekcja i storage

Audit parsuje wyłącznie exact bieżący `GatekeeperBuyLog` schema v33. Unknown,
arbitralny lub defaultowany row nie może być denominator baseline. Dla każdego
runu wymagane jest:

```text
current_v33_identities == v34_identities == evidence_identities
```

Denominator storage powstaje dopiero po exact identity join, zgodności
v33/manifest config provenance i poprawnym replayu. Normatywna formuła to:

```text
additional_storage_delta_ratio =
    (paired_v34_total_bytes + paired_sidecar_total_bytes)
    / paired_v33_total_bytes
```

Nie występuje odejmowanie `1.0` ani testowy padding v33.

### 2.2 Pełny resource timer

Jeden ciągły production timer obejmuje:

```text
start bezpośrednio przed pierwszym canonical producer call
→ wszystkie canonical producers raz
→ full evidence + semantic validation
→ compact projection + semantic validation + Wire gate + canonical hash
→ terminal pair construction
→ writer-owned timestamp/part binding
→ final evidence/v34 JSON bytes
→ pojedynczy odczyt elapsed w writerze
```

Transportowe timestamp i part index są wyłączone z semantic evidence hash.
Writer nie wykonuje ponownie family validation ani evidence JCS/SHA-256 tylko
po to, aby je przypisać. Deserializacja i replay nadal niezależnie wykonują
pełną validation/hash verification na swoich trust boundaries.

Ten sam `Instant` jest przenoszony przez snapshot, comparator i pair; nie jest
to suma niezależnych segmentów. Filesystem I/O, queue wait i backpressure
pozostają osobnymi metrykami.

### 2.3 Policy drift jako trwały wynik

Niezerowa `equivalence_deltas` nie jest structural pair error. Writer zapisuje
v34 oraz sidecar, replay je odtwarza, a audit klasyfikuje rekord jako
`FAIL_POLICY_DRIFT`. Brak drugiej porównywalnej decyzji daje wszystkie pola
`NotEvaluable`, nigdy fikcyjne `Equal`.

Nowszy amendment utrwala w semantic evidence hash authoritative/comparator
normalized snapshots, comparator evaluability, policy version i Gatekeeper
config hash. Replay recomputuje exact deltas i odrzuca ich zmianę w v34.

Regresja używa rzeczywistego `evaluate_policy_from_assessment()`, wprowadza
dokładnie jedną różnicę primary reason, finalizuje paired manifest i wymaga
`FAIL_POLICY_DRIFT`.

### 2.4 Niezależny durable cutoff

`MetricContractEvidenceHashPayloadV1` zawiera teraz
`MetricContractDecisionSourceCutoffV1`. Cutoff jest częścią semantic evidence
hash. Replay buduje oczekiwany context z durable evidence, nie z projection
podlegającej walidacji, i wymaga exact equality wszystkich surface cutoffów.

### 2.5 Replay v34 semantics i counterfactual diagnostics

Replay odbudowuje i porównuje:

- `authoritative_contracts` z frozen Profile A;
- `comparator_contracts` z frozen Profile A;
- `measured_fields_mask` z manipulation projection;
- evaluability i delta dev-primary;
- evaluability i delta corrected FTDI actionability;
- `counterfactual_delta_present`.

Lane z `Null` po którejkolwiek stronie jest `NotEvaluable`, nie różnicą.
Rzeczywista delta emituje typed diagnostic:

```text
COUNTERFACTUAL_POLICY_DELTA_OBSERVED:<lane>:<run>:<join>:<plane>
```

Diagnostyka nie zmienia authoritative verdictu ani authority assignment.

### 2.6 Bundle i BURN binding

Każdy finalized part manifest zawiera:

- exact 40-hex build commit i `build_worktree_clean`;
- Gatekeeper i brain config hashes;
- rollout mode;
- decision/evidence/projection/Wire schema versions;
- Wire V1 codebook BLAKE3;
- `BURN_IN_CONTRACT_V2` version i canonical hash;
- profile i exact effective-config provenance.

Bundle wymaga unikalnych `run_id`, globalnie unikalnych pełnych record
identities, zgodnego provenance i niepokrywających się zakresów czasowych.
UTC buckets pochodzą ze wszystkich poprawnie sparowanych durable cutoffów.
Clean Flip wymaga `Available + Measured + eligible_buyer_count > 0`. Real dev
divergence wymaga dwóch obecnych wartości; `None != Some` nie jest evidence.

Każdy run musi uzyskać `PASS_CUTOVER_READY` przed agregacją minimów frozen
contractu. Rows niepóźniejsze niż `frozen_at` są wykluczone.

V2 jednoznacznie zastępuje wcześniejszy pre-run draft V1 po autoryzowanej
zmianie limitu z 1 ms do 5 ms. Wszystkie parts jednego runu muszą dodatkowo
mieć exact frozen provenance, a histogramy przechodzą closed codebook/count/max
validation.

### 2.7 Durable writer i rotation

Writer nadaje timestamp na rzeczywistej writer boundary. Part data używa
`sync_data()`. Final manifest używa temp file, `sync_all()`, rename oraz
directory `sync_all()`. `writer_finalized=true` nie pozostaje po failure.

Counters obejmują również manifest/finalization failures. Fault matrix zawiera
obie orientacje orphanów oraz prawdziwy mid-row short write, który zapisuje
prefiks JSONL przed błędem. Audit sprawdza first/last identity, run identity,
evidence part index, writer timestamp, confinement ścieżki, SHA, bytes, rows i
ciągłość parts.

## 3. Provenance kompilacji

`ghost-brain/build.rs` nie honoruje arbitralnego `GIT_COMMIT` z environment.
Commit pochodzi z `git rev-parse HEAD`; błąd inspekcji daje jawne
`unknown_build_commit`, które audit odrzuca. Clean bit pochodzi z
`git status --porcelain --untracked-files=all`; błąd inspekcji oznacza dirty.
Build script obserwuje źródła workspace, HEAD i index, aby nie zachować stale
clean result po zmianie kodu.

## 4. CI i weryfikowalność

Dodany workflow `.github/workflows/metric-contracts-pr2c.yml` uruchamia pełne
PR2A/PR2B/PR2C suites, frozen Gatekeeper/lifecycle regressions, checks,
targeted Clippy oraz osobny release full-path resource harness. Workflow nie
uruchamia runtime live/shadow ani external RPC. Correctness job instaluje
jawnie `ripgrep`, ponieważ zamrożone PR2A/PR2B static guards używają `rg` i nie
mogą zależeć od przypadkowego obrazu runnera.

## 5. Wyniki resource gate

Po usunięciu wielokrotnej walidacji niezmiennego profile/effective-config z
per-decision hot path release harness na tej samej funkcji co materializacja
runtime uzyskał:

Niezmienny context jest reprezentowany przez opaque
`MetricDecisionProjectionValidatedStaticContextV1`. Proof powstaje wyłącznie
po pełnej walidacji profile/effective-config, wiąże exact immutable references,
a dynamiczny source cutoff jest nadal walidowany dla każdego terminalnego
snapshotu. Publiczne i replayowe trust boundaries zachowują pełną walidację.

- full build+serialize p50/p95/p99/max: `3 545 / 3 545 / 3 545 / 3 545 us`;
- complete snapshot p50/p95/p99: `1 558 / 1 963 / 2 267 us`;
- projection build/validate/hash p50/p95/p99: `476 / 669 / 733 us`;
- terminal pair construction p50/p95/p99: `373 / 509 / 630 us`;
- final serialization diagnostic p50/p95/p99: `43 / 67 / 79 us`;
- comparator p50/p95/p99: `5 / 8 / 21 us`;
- Wire V1 p95/max: `2 339 / 2 339 B`;
- sidecar p95/p99: `22 180 / 22 180 B`;
- v34 p95: `1 176 B`.

Autoryzowany amendment z 2026-07-13 podniósł pełny build+serialize i projection
build/validate z 1 ms do 5 ms p99. Comparator i logger enqueue pozostają
ograniczone do 1 ms. Final serialization jest diagnostycznym podetapem
objętym ciągłym full-path gate, a nie drugim nakładającym się gate. Timer nie
został skrócony ani przesunięty za producer boundary.

## 6. Zakres zamrożony

Bez zmian pozostają:

- Gatekeeper V2/V2.5/V3 thresholds, weights, phases, soft points, verdicts i
  primary reasons;
- authority Profile A;
- DecisionLogger v33 schema;
- V3 replay v1;
- selector score, IWIM, sender, Jito, execution i post-buy;
- active dev source oraz live/shadow boundary;
- rollout `Legacy`.

Nie rozpoczęto PR3 ani Type-5 T1. Nie aktywowano DualCompute ani V2 rollout.

## 7. Konsekwencje

Pozytywne:

- structural integrity nie usuwa już evidence o policy drift;
- cutoff, BURN contract i build provenance mają niezależny durable anchor;
- single-run i bundle audit są fail-closed wobec braków/nadmiarów;
- resource PASS jest mierzony na tej samej ścieżce co production writer;
- crash/short-write failures pozostają wykrywalne.

Koszt:

- większy manifest i bardziej rygorystyczne odrzucanie historycznych lub
  niepełnych danych;
- dodatkowe fsync przy finalizacji/part updates;
- dirty/unknown build nie może wejść do prospective burn-in.

## 8. Decyzja

Ten historyczny amendment zamknął pierwszą serię blockerów, lecz jego markery
readiness zostały później wycofane przez kolejne review. Bieżący stan i
obowiązujący `BURN_IN_CONTRACT_V3` opisuje trzeci amendment; poniższy blok jest
zachowany wyłącznie jako historyczny zapis decyzji V2 i nie stanowi aktualnego
prospective-burn-in acceptance.

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
BURN_IN_CONTRACT_V2_FROZEN
GATEKEEPER_POLICY_UNCHANGED
V3_V1_REPLAY_UNCHANGED
TYPE5_NOT_STARTED
METRIC_CONTRACTS_V1_1_DUAL_COMPUTE_READY_FOR_PROSPECTIVE_BURN_IN
PR2C_READY_FOR_REVIEW
```
