# Raport P3 V3 Calibration Gate - 2026-05-15

## Decyzja

P3 Calibration Gate zostaje uruchomiony jako analiza offline na zamrozonym P1
baseline. Wynik pierwszego gate'a: `INSUFFICIENT_DATA`.

To nie cofa P1. P1 runtime evidence pozostaje `APPROVED`:

- baseline commit: `0f3639c Close V3 P1 remediation and runtime evidence`
- `status=ok`
- `v3_rows=86`
- `artifact_freshness.stale_against_config=false`
- `replay_status=hash_only`

P2 promotion nie jest zatwierdzony ani uruchomiony. V3 pozostaje shadow sidecar.

## Zakres P3

In scope:

- porownanie swiezego P1 runu z historycznymi runami P1/P0,
- hash/config matrix,
- reason distributions,
- cross-run reason stability,
- confidence bucket diagnostics,
- ablation proxy per V3 reason group,
- osobna ocena dominujacego `REJECT_V3_MANIPULATION_CONTRADICTION`.

Out of scope:

- promocja V3 do active policy,
- zmiana active BUY/REJECT/TIMEOUT behavior,
- zmiana progow V2/V2.5,
- zmiana IWIM lub execution/live sender,
- traktowanie `hash_only` jako full replay OK.

## Narzedzie

Dodano offline gate:

`scripts/v3_replay_ablation_report.py` jest offline-only i nie dotyka runtime
path. Jawnie podane `--decisions-log` i `--compare-decisions-log` fail-closed,
jesli plik nie istnieje, zeby P3 nie produkowal mylacego porownania na pustym
zbiorze.

`--shadow-lifecycle` i `--events-dir` sa zarezerwowane dla przyszlego merge
lifecycle/event replay. W obecnym P3 fail-closed, jesli operator je poda, zeby
nie sugerowac, ze outcome labels zostaly uwzglednione w obliczeniach.

```bash
python3 scripts/v3_replay_ablation_report.py \
  --config configs/rollout/shadow-burnin.toml \
  --compare-decisions-log logs/rollout/shadow-burnin-v3-p1.20260515T111441Z.pre-rerun/decisions/shadow-burnin-v3-p1/v2.5/v25_shadow/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl \
  --compare-decisions-log logs/rollout/shadow-burnin-v25-repair-r2/decisions/shadow-burnin-v25-repair-r2/v2.5/v25_shadow/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl \
  --json
```

Test kontraktowy:

```bash
python3 -m unittest scripts/test_v3_replay_ablation_report.py -v
```

## Inputy

Primary P1 baseline:

- `logs/rollout/shadow-burnin-v3-p1/decisions/shadow-burnin-v3-p1/v2.5/v25_shadow/eb9f928e8c86d717aeac49a118fe3e0fa5cd094c9ecc16ad09d371ad54b0e581/gatekeeper_v2_decisions.jsonl`
- rows: `86`
- `v3_policy_config_hash` missing: `0`
- `v3_feature_snapshot_hash` missing: `0`
- duplicate `ab_record_id` conflicts: `0`

Compare 1, historical P1:

- `logs/rollout/shadow-burnin-v3-p1.20260515T111441Z.pre-rerun/decisions/shadow-burnin-v3-p1/v2.5/v25_shadow/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl`
- rows: `29`

Compare 2, older P0/P1-adjacent repair run:

- `logs/rollout/shadow-burnin-v25-repair-r2/decisions/shadow-burnin-v25-repair-r2/v2.5/v25_shadow/05d5df619448d740abf4e6cde740d027837b7d75bc091cf93f078354faf29f68/gatekeeper_v2_decisions.jsonl`
- rows: `141`
- note: older rows carry V3 shadow fields but do not carry current P1 policy/snapshot hashes.

## Replay i hashe

P3 gate returned:

- `status=ok`
- `replay.status=hash_only`
- `replay.rows=86`
- `replay.policy_hash_missing=0`
- `replay.snapshot_hash_missing=0`
- `replay.full_snapshot_payload_rows=0`
- `replay.duplicate_ab_record_conflict_count=0`

Interpretacja: P1 ma pelne hash coverage i brak konfliktowych duplikatow, ale nie
ma pelnego payloadu `MaterializedFeatureSet` w JSONL. Dlatego P3 moze robic
hash/config/reason analysis, ale nie moze uczciwie zadeklarowac full replay parity.

## Reason Distribution

Primary P1 baseline, 86 rows:

- `REJECT_V3_MANIPULATION_CONTRADICTION`: `70`
- `PENDING_V3_WAIT_EVIDENCE`: `13`
- `PENDING_V3_WAIT_SAMPLE`: `3`

Historical P1, 29 rows:

- `REJECT_V3_MANIPULATION_CONTRADICTION`: `20`
- `PENDING_V3_WAIT_EVIDENCE`: `7`
- `PENDING_V3_WAIT_SAMPLE`: `2`

Older repair run, 141 rows:

- `REJECT_V3_MANIPULATION_CONTRADICTION`: `96`
- `PENDING_V3_WAIT_EVIDENCE`: `38`
- `PENDING_V3_WAIT_SAMPLE`: `6`
- `REJECT_V3_LOW_ORGANIC_BROADENING`: `1`

`REJECT_V3_MANIPULATION_CONTRADICTION` dominuje we wszystkich trzech zestawach:
`70/86`, `20/29`, `96/141`. To jest stabilny sygnal diagnostyczny, ale nie jest
jeszcze dowod, ze bucket jest poprawnie skalibrowany.

## Cross-Run Stability

Primary P1 vs historical P1:

- shared reasons:
  - `PENDING_V3_WAIT_EVIDENCE`
  - `PENDING_V3_WAIT_SAMPLE`
  - `REJECT_V3_MANIPULATION_CONTRADICTION`
- reason Jaccard: `1.0`

Primary P1 vs older repair run:

- shared reasons:
  - `PENDING_V3_WAIT_EVIDENCE`
  - `PENDING_V3_WAIT_SAMPLE`
  - `REJECT_V3_MANIPULATION_CONTRADICTION`
- right-only:
  - `REJECT_V3_LOW_ORGANIC_BROADENING`
- reason Jaccard: `0.75`

Interpretacja: reason set jest stabilny miedzy dwoma P1 runami. Starszy repair run
dodaje pojedynczy organic broadening reject, wiec nie jest identyczny semantycznie
z P1 baseline i nie powinien byc traktowany jako bezposredni promotion proof.

## Calibration Buckets

Primary P1 baseline:

- populated confidence bucket: `0`
- bucket `0` count: `86`
- active verdict distribution: `REJECT=86`
- V3 verdict distribution: `REJECT=70`, `PENDING=16`
- outcome label coverage: `0.0`
- unknown outcome ratio: `1.0`
- degraded evidence ratio: `0.976744`

Interpretacja: P3 nie ma jeszcze outcome/lifecycle labels pozwalajacych ocenic
jakosc kalibracji procentowo. Bucket `0` mowi, ze obecny V3 shadow byl skrajnie
konserwatywny w tym runie; to jest dobre dla shadow safety, ale niewystarczajace
do promocji lub strojenia bez dodatkowych etykiet.

## Ablation Proxy

P3 nie wykonuje kontrfaktycznego recompute scoringu, bo replay jest `hash_only`.
Ablation jest jawnie oznaczony jako `reason_group_proxy`.

Reason group counts:

- `manipulation_contradiction`: `70`
- `evidence_wait`: `13`
- `organic_broadening`: `3`

Variant proxy:

- `no_manipulation_contradiction`: `changed_rows_proxy=70`
- `no_organic_broadening`: `changed_rows_proxy=3`
- `no_sybil_fsc_cpv_caps`: `changed_rows_proxy=0`
- `no_alpha_cap`: `changed_rows_proxy=0`
- `no_execution_cap`: `changed_rows_proxy=0`

Interpretacja: obecny P1 baseline jest w praktyce testem jednego dominujacego
mechanizmu - `manipulation_contradiction`. Inne grupy nie daja jeszcze
wystarczajacego sygnalu w primary runie, zeby ocenic ich marginalny wklad.

## Ocena `REJECT_V3_MANIPULATION_CONTRADICTION`

Ten bucket wymaga osobnej walidacji przed jakakolwiek promocja:

- dominuje `70/86` w swiezym P1 baseline,
- dominuje rowniez w historycznym P1 i starszym repair runie,
- ablation proxy wskazuje, ze usuniecie tej grupy dotkneloby `70` rows,
- `hash_only` nie pozwala sprawdzic kontrfaktycznego score bez pelnego snapshotu,
- brak outcome labels oznacza brak dowodu, czy te rejecty sa true-negative,
  overfit, albo zbyt agresywnym bucketem.

Wniosek: bucket jest waznym kandydatem sygnalowym, ale P3 blokuje promotion-ready
status do czasu pelnego replay/outcome evidence lub targeted manual audit.

## Certification

```text
p3_status=insufficient_data
promotion_ready_gates=[]
blocked_gates=[]
insufficient_evidence_gates=[
  dominant_manipulation_contradiction_requires_more_evidence,
  low_outcome_label_coverage,
  replay_hash_only
]
no_p2_promotion=true
```

Runtime contract:

- `active_policy_changed=false`
- `promotion_activated=false`
- `decision_plane_v3_shadow_created=false`

## Werdykt Operacyjny

P1 zostaje zamrozony jako audytowalny baseline.

P3 Calibration Gate pierwszego przebiegu: `INSUFFICIENT_DATA`.

Nie ma podstaw do P2 promotion. Nastepny poprawny krok to zebranie dodatkowego
P3 evidence:

- pelny replay payload albo deterministyczna rekonstrukcja `MaterializedFeatureSet`,
- outcome/lifecycle labels dla bucketow confidence,
- targeted audit `REJECT_V3_MANIPULATION_CONTRADICTION`,
- kolejne swieze runy na tym samym policy hash,
- porownanie reason stability po wiekszej probie.
