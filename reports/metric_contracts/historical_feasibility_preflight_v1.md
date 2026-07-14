# PR0: historical feasibility preflight kontraktów metryk

Status:

```text
FEASIBILITY_PREFLIGHT_COMPLETE
FEASIBILITY_ONLY
V2_DIMENSIONS_NOT_MEASURABLE_PRE_IMPLEMENTATION
NOT_VALIDATION_EVIDENCE
PROVENANCE_AND_REPRODUCIBILITY_PASS
```

Data audytu: 2026-07-11

Audytowany code commit:
`f3318f3a71a9202ced7af9cf43c064fa9f2f0c4a`

Base i merge-base PR #60:
`f1e3292aae935d1b43e2c265c078f9ec74a62563`

Tree obu commitów:
`92e97058349157b591a24f11da3bec0642051cd7`
(`TREE_EQUIVALENCE_PASS`; exact commands w `pr0_reproduction_v1.md`).

Plan normatywny:
`PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`

## 1. Cel i zasada anty-post-hoc

Preflight odpowiada wyłącznie na pytanie, które minima można rozsądnie zamrozić
przed przyszłą walidacją i czy obecna infrastruktura ma wystarczającą skalę.
Historyczne rekordy otrzymują klasę `FEASIBILITY_ONLY` i nigdy nie zwiększają
prospective validation counts.

Nie obniżono żadnego minimum po zobaczeniu niekorzystnego wyniku. Zgodnie z
decyzją właściciela planu minima można kontrolowanie potwierdzić lub zmienić
wyłącznie przed zebraniem danych walidacyjnych. Ten PR0 nie zamraża
`BURN_IN_CONTRACT_V1`, ponieważ producenci flip-v2/dev-primary oraz audit CLI
jeszcze nie istnieją. Zamrożenie nastąpi dopiero w PR2C, przed nowymi runami.

## 2. Klasy danych i snapshot discipline

Znaleziono pięć historycznych run directories z v33 Gatekeeper decision JSONL.
Cztery zakończone runy zostały przeskanowane jako stabilny feasibility set.
Piąty run r5 był nadal obsługiwany przez żywy proces
`target/release/ghost-launcher` (PID 2506429 podczas audytu), a jego decision
file zwiększył liczbę rekordów między dwoma wcześniejszymi przebiegami skanu.

Dlatego:

```text
r1/r2/r3/r4 = STABLE_FEASIBILITY_INPUT
r5          = ACTIVE_MUTABLE_NOT_IMMUTABLE
```

r5 został wyłączony z SHA/count aggregate. Chwilowy brak wzrostu w trzysekundowym
oknie nie zmienia klasy: działający writer i wcześniejszy przyrost oznaczają, że
plik nie jest immutable. Nie kopiowano ani nie zatrzymywano aktywnego runa.

## 3. Stabilny run inventory

Nazwy runów nie są źródłem prawdy dla czasu. Duration wyliczono z minimalnego i
maksymalnego RFC3339 `timestamp` w decision rows.

| Run | Observed UTC range | Duration h | Rows | Dev-known | Legacy flip present | Size bytes | SHA-256 |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| r2 `human-ab-stage2-exec-exit-8h` | 2026-07-08 21:25:12.799298 – 2026-07-09 05:24:47.708750 | 7.9930 | 8,316 | 7,476 | 7,467 | 851,829,798 | `231ed686ae3e113aea28486cafa94bc9118c9186df28e2422068dfbf9165d444` |
| r1 `human-ab-thresholds-4h` | 2026-07-08 03:43:07.416286 – 19:46:59.952349 | 16.0646 | 10,459 | 9,551 | 9,532 | 1,078,037,357 | `0264a0f9db9faf8943a9aa48f5185211f1ccba4daaef8bbbca35494f61728302` |
| r4 `regime-depth-oos-6h` | 2026-07-10 00:50:55.818010 – 08:40:17.736359 | 7.8228 | 5,797 | 5,228 | 5,216 | 593,150,549 | `54f73953cb1ffa650f57adf5423bc41887b61d49a7e3547cbc8af50c2b1cacba` |
| r3 `stage1-buylike-replication-6h` | 2026-07-09 16:33:26.028716 – 20:55:38.538291 | 4.3701 | 6,694 | 6,234 | 6,228 | 697,297,228 | `b6ec3b1ffc0583b9fd4b59ebc4ee241119ef53023813fba3e43ef8dd8e500238` |

Wszystkie cztery pliki znajdują się pod:

```text
logs/rollout/<run>/decisions/<run>/v2.2/legacy_live/
d0480c9b7b3c26e42918c60833d34c016d2b0188182754ba9a64f52086d80c22/
gatekeeper_v2_decisions.jsonl
```

W każdym runie wszystkie rows mają `log_schema_version=33`,
`decision_plane=legacy_live` i powyższy wspólny Gatekeeper `config_hash`.
`brain_config_hash` różni się per run, zgodnie z różnymi research overlays.
Sama ta różnica nie dyskwalifikuje bundle. Historyczne rows nie emitują jednak
`metric_contract_effective_config_hash`, więc nie da się ustalić, czy wszystkie
resolved producer/population/dedupe/dust/window/status/comparator settings były
identyczne. Config equivalence ma status `NOT_MEASURABLE_PRE_IMPLEMENTATION`.

## 4. Aggregate stable feasibility set

| Właściwość | Wynik |
| --- | ---: |
| Run count | 4 |
| Sum observed duration | 36.2505 h |
| Decision rows | 31,266 |
| Unique record identities `(run_id, join_key, decision_plane)` | 31,266 |
| Missing record identity rows | 0 |
| Duplicate record identities within/across inputs | 0 / 0 |
| Cross-run join-key collisions observed, diagnostic only | 0 |
| Stable underlying-event identity rows | 0 |
| Cross-run underlying-event collision gate | `NOT_MEASURABLE_PRE_IMPLEMENTATION` |
| Malformed/truncated JSON rows | 0 |
| Dev-known rows | 28,489 (91.12%) |
| Legacy `flip_ratio_10s` present | 28,443 (90.97%) |
| FSC v2 payload present | 31,266 (100%) |
| Materialized snapshot present | 31,266 (100%) |
| V3 v1 full replay input by inspected field contract | 31,266 (100%) |
| Gatekeeper V2 v1 strict replay-ready rows | 23,993 (76.74%) |
| Total bytes | 3,220,314,932 bytes (3.220 GB / 2.999 GiB) |

Observed throughput wynosi około 862.50 decisions/h. Zagregowany historyczny
storage rate to 0.0888 GB/h (84.72 MiB/h), liczony jako total bytes / suma
observed durations. Nie jest to obietnica wydajności dla v34; stanowi baseline
do limitu wzrostu z planu.

## 5. Record size baseline

Percentyle obliczono na długości każdej linii bez końcowego newline, dla 31,266
stabilnych v33 records:

| Percentyl | Bytes/record |
| --- | ---: |
| min | 83,064 |
| p50 | 103,380 |
| p95 | 119,890 |
| p99 | 137,102 |
| max | 230,540 |
| mean | 102,996.34 |

Per-run p99 mieści się między 131,242 i 140,297 bytes. To potwierdza decyzję,
aby nie dodawać dziesięciu pełnych dual payloadów do głównego rekordu v34.
PR2C musi zmierzyć compact summary i osobny typed sidecar, queue high-water,
serialization latency, writer failures, drops, rotations i GB/hour delta.

## 6. Replay readiness i obserwowalność

V3 field contract był kompletny we wszystkich stabilnych rows: schema v1,
materialized snapshot, policy config i feature snapshot hash były obecne.

Gatekeeper V2 strict replay readiness wynosi 23,993/31,266. Pozostałe 7,273
rows miały jednolity stan:

```text
gatekeeper_v2_replay_input_schema_version = 1
gatekeeper_v2_replay_ready_non_temporal   = false
gatekeeper_v2_replay_ready_temporal       = true
gatekeeper_v2_config_payload              = present
gatekeeper_decision_payload               = present
gatekeeper_v2_replay_incomplete_reason    = null
```

To jest luka obserwowalności: bool mówi, że non-temporal replay nie jest ready,
ale typed/string reason nie wyjaśnia dlaczego. Historyczne runy nie spełniają
planowanego 100% full Gatekeeper V2 replay gate i nie mogą zostać uznane za
validation bundle. PR2C musi wymagać jawnego incomplete reason i fail closed.

## 7. Manifest, reprodukcja i immutability audit

Istniejące `reports/selector/<run>/pre_run_manifest.json` są manifestami fazy
`pre_run`, a nie końcowymi manifestami decision logs:

- r2/r3/r4 mają `artifact_count=0` i `total_size_bytes=0`;
- r1 ma `run_id=UNKNOWN` oraz trzy małe overlay entries, nie decision JSONL;
- żaden nie zawiera SHA, row count, rotated parts ani schema coverage badanego
  `gatekeeper_v2_decisions.jsonl`.

PR0 uzupełnia tę historyczną lukę trzema repo artifacts:

```text
reports/metric_contracts/pr0_input_manifest_v1.json
reports/metric_contracts/pr0_feasibility_summary_v1.json
reports/metric_contracts/pr0_reproduction_v1.md
```

Manifest utrwala path/basename, SHA-256, bytes, rows, min/max timestamp, schema,
run ID, Gatekeeper/brain hashes i immutable/mutable classification. Summary jest
bezpośrednim wynikiem wersjonowanego skanera. Reproduction doc zawiera pełne
źródło skanera, jego SHA, wersje narzędzi i exact commands.

Skaner ponownie odczytał wszystkie 3.220 GB i zwrócił
`input_validation.status=PASS`, bez mismatchów. Exact generated JSON jest
identyczny z checked summary. Surowych plików nie dodano do Git, więc reprodukcja
wymaga tych samych content-addressed inputs. Te artifacts nie zastępują
runtime-generated post-run manifestu i nie promują danych ponad
`FEASIBILITY_ONLY`.

## 8. Czy obecny payload odtwarza dev-primary i flip-v2?

Skan wszystkich 31,266 stabilnych rows wykazał:

| Wymagane provenance | Rows present | Werdykt |
| --- | ---: | --- |
| pool/create signature | 0 | `NOT_MEASURABLE_PRE_IMPLEMENTATION` |
| raw transaction sequence w materialized snapshot | 0 | `NOT_MEASURABLE_PRE_IMPLEMENTATION` |
| tx key/index/event ordinal/source order provenance | 0 | `NOT_MEASURABLE_PRE_IMPLEMENTATION` |

Konsekwencje:

- `dev_wallet_known` i legacy first-observed dev field można policzyć, ale nie
  da się rzetelnie odtworzyć create-signature primary creator buy ani prawdziwej
  legacy/v2 dev divergence;
- obecność legacy `flip_ratio_10s` nie oznacza flip-v2 evaluability. Bez raw
  ordered eligible events, stable identity, success/dust status i per-owner
  state nie da się odtworzyć normatywnego V2 automatu;
- nie wolno zastępować tych braków proxy ani zakładać, że legacy present = V2
  clean/evaluable.

## 9. Ocena początkowych minimów

Początkowa hipoteza z planu:

```text
8 h aggregate duration
700 unique decisions
100 dev-known
100 clean flip-v2 evaluable
30 real dev legacy/v2 divergences
```

| Minimum | Historycznie mierzalne? | Feasibility result | Decyzja PR0 |
| --- | --- | --- | --- |
| 8 h aggregate | tak | 36.25 h w 4 stabilnych runach | skala wykonalna |
| 700 unique decisions | tak | 31,266 | skala wykonalna |
| 100 dev-known | tak | 28,489 | skala wykonalna |
| 100 clean flip-v2 evaluable | nie | brak V2 producer/raw provenance | `NOT_MEASURABLE_PRE_IMPLEMENTATION` |
| 30 real dev divergences | nie | brak create signature/raw order | `NOT_MEASURABLE_PRE_IMPLEMENTATION` |

Warunek co najmniej trzech runów i dwóch UTC 4h buckets jest historycznie
osiągalny ilościowo. Nie oznacza PASS bundle: brak effective-config hash i final
manifests, stable event identity jest nieobecne, a Gatekeeper V2 replay jest
niepełny. Różne pełne brain config hashes są provenance, nie samodzielnym FAIL.

## 10. Wniosek dla `BURN_IN_CONTRACT_V1`

PR0 nie zamraża liczb ani nie zmienia hipotezy. Najrzetelniejsza decyzja brzmi:

```text
duration/decision/dev-known scale = FEASIBLE
flip-v2 evaluability             = NOT_MEASURABLE_PRE_IMPLEMENTATION
dev divergence coverage          = NOT_MEASURABLE_PRE_IMPLEMENTATION
BURN_IN_CONTRACT_V1              = NOT_YET_FROZEN
```

Po PR2B i PR2C należy wykonać kontrolowany historical feasibility audit na
referencyjnych V2 producers. Dopiero wtedy właściciel planu zatwierdza exact
minima, a contract otrzymuje version/hash/`frozen_at`. Wszystkie qualifying
validation rows muszą mieć timestamp po `frozen_at`.

Jeśli po starcie validation minimum okaże się niewygodne, nie wolno go obniżyć.
Zmiana wymaga `INVALIDATED_BY_GATE_CHANGE`, nowej wersji/hash/`frozen_at` i
całkowicie nowych prospective runów.

## 11. Wymagany następny pomiar w PR2C

Audit CLI musi przed freeze wygenerować co najmniej:

- per-run i aggregate duration/unique decisions/dev-known;
- clean flip-v2 evaluable count według typed status envelope;
- real dev legacy/v2 feature divergence count;
- coverage wszystkich 10 contracts i status/reason distributions;
- full replay completeness oraz dokładne incomplete reasons;
- build/profile/config/schema hashes;
- `metric_contract_effective_config_hash` obejmujący wszystkie resolved settings
  wpływające na producer/population/dedupe/dust/window/status/comparator;
- pełny `brain_config_hash` jako provenance bez automatycznego equality gate;
- duplicate full record identity oraz osobny stable underlying-event collision
  result;
- p50/p95/p99 bytes per compact decision i full evidence record;
- GB/hour delta względem tego v33 baseline;
- serialization/build/enqueue p99, queue high-water, dropped rows, writer
  failures i missing summary-sidecar pairs;
- rotated parts manifest, SHA i zero overlaps/duplicates.

## 12. Końcowy werdykt

```text
FEASIBILITY_PREFLIGHT_COMPLETE
FEASIBILITY_ONLY
V2_DIMENSIONS_NOT_MEASURABLE_PRE_IMPLEMENTATION
NOT_VALIDATION_EVIDENCE
PROVENANCE_AND_REPRODUCIBILITY_PASS
```

Ten wynik jest pozytywnym zakończeniem PR0, ponieważ brakujące wymiary zostały
oznaczone zgodnie z planem, a nie uzupełnione zgadywaniem. Nie jest to
`PASS_EVIDENCE_CONSISTENT`, `NOT_EVALUABLE` prospective bundle ani zgoda na policy
promotion.
