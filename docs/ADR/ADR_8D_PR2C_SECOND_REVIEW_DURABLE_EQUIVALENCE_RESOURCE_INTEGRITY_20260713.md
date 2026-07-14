# ADR-8D: PR2C — durable equivalence replay i integralność resource/BURN

Status: `SUPERSEDED BY 2026-07-14 FINALIZATION / HISTORICAL IMPLEMENTATION RECORD`

Typ: ADR-8D / second review amendment / durability / replay / resource metrology

Data: 2026-07-13

Repo: `smahacfel/Gho`

Branch: `agent/metric-contract-pr2c-durable-evidence-replay`

Base i merge-base: `fc87f288651ebd1b5ec8eb7f6660e85f8fd294d9`

Amendowane dokumenty:

- `docs/ADR/ADR_8D_PR2C_METRIC_CONTRACT_DURABLE_EVIDENCE_REPLAY_20260713.md`;
- `docs/ADR/ADR_8D_PR2C_REVIEW_BLOCKERS_DURABILITY_AUDIT_20260713.md`;
- `PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`.

Raport dowodowy:
`reports/metric_contracts/pr2c_durable_evidence_replay_verification_v1.md`

Poziom ryzyka: `HIGH`. Amendment dotyka monotonicznego pomiaru terminalnej
ścieżki, semantic evidence hash, replayu equivalence lane, rotation manifestu
i prospective BURN audit. Nie zmienia aktywnej Gatekeeper policy, authority
Profile A, terminalnego verdictu ani rollout mode `Legacy`.

Uwaga normatywna: decyzje tego dokumentu o limitach 1/5 ms,
`BURN_IN_CONTRACT_V2` i prospective readiness zostały wycofane przed
jakimkolwiek prospective row. Finalny PR2C raportuje latency wyłącznie
diagnostycznie, jest default OFF i nie autoryzuje burn-inu.

## 1. Problem

Drugie blocking review wykazało, że część wcześniejszych dowodów była zbyt
wąska mimo poprawnej podstawowej implementacji:

1. histogram `metric_contract_build_and_serialize_us` zaczynał się po
   canonical producer boundary i składał niezależne odcinki czasu;
2. `equivalence_deltas` były trwałe, lecz durable evidence nie zawierało
   niezależnych authoritative/comparator snapshots do ich recompute;
3. audit ufał histogramom manifestu bez zamkniętej walidacji codebooku,
   liczności próbek i `max_us`;
4. provenance było porównywane między summary/evidence tego samego partu, lecz
   nie pomiędzy wszystkimi partami jednego runu;
5. plan jednocześnie zawierał limity 1 ms i 5 ms, a dwa różne payloady były
   określane jako `BURN_IN_CONTRACT_V1`;
6. semantyka UTC bucketów i `brain_config_hash` różniła się między kodem a
   planem, a raport PASS nie miał typed `cutover_scope`.

## 2. Historyczna decyzja timera — wycofana

Ten etap wprowadził jeden ciągły timer i tymczasowy latency gate. Finalizacja
2026-07-14 zachowuje timer wyłącznie jako diagnostykę p50/p95/p99/max i usuwa
wszystkie latency thresholds z merge/burn-in acceptance.

Normatywna granica pełnego pomiaru jest teraz realizowana przez jeden
`std::time::Instant`:

```text
bezpośrednio przed pierwszym canonical producer call
→ wszystkie canonical producer snapshots
→ full evidence + semantic validation
→ compact projection + validation + semantic hash
→ authoritative normalization + rzeczywisty comparator
→ terminal pair construction
→ writer-owned timestamp/rotation binding
→ exact final evidence JSON bytes
→ exact final v34 JSON bytes
→ odczyt elapsed i zapis próbki histogramu
```

`Instant` jest runtime-only: nie wchodzi do v34, evidence payloadu, projection,
serde ani semantic hash. Writer nie sumuje osobnych timerów. Regresja wstawia
celową przerwę między pair boundary i writerem; próbka musi tę przerwę zawierać.

## 3. Durable equivalence evidence

Zahashowany `MetricContractEvidenceHashPayloadV1` zawiera teraz
`MetricContractPolicyEquivalenceEvidenceV1`:

- policy version;
- exact Gatekeeper config hash;
- comparator evaluability;
- authoritative normalized policy snapshot;
- comparator normalized policy snapshot.

Każdy snapshot zachowuje verdict, primary reason, ordered reason chain,
sześciopolowy phase vector, soft points, selector soft score i hard-fail class.
Replay wylicza exact `equivalence_deltas` ponownie z durable snapshots i wymaga
równości z v34. Mutacja `Different → Equal` przy niezmienionym evidence hash
kończy się `FAIL_SCHEMA_OR_REPLAY`/`SummarySemanticMismatch`, a rzeczywisty
drift pozostaje poprawnym trwałym rekordem klasyfikowanym przez audit jako
`FAIL_POLICY_DRIFT`.

## 4. Integralność histogramów

Każdy z trzech histogramów manifestu przechodzi typed validation:

```text
bucket_upper_bounds_us == frozen codebook
checked_sum(bucket_counts) == sample_count
sample_count == paired_commands_total
max_us należy do najwyższego niepustego bucketu
overflow bucket jest spójny z max_us i ostatnim boundem
```

Przy zerowych drop/send/writer failures także enqueue histogram ma dokładnie
jedną próbkę na zaakceptowany paired command. Audit odrzuca mutacje bounds,
sample count, bucket sum, max oraz brak pojedynczej próbki przy poprawnych rows.

## 5. Jedno frozen provenance dla całego runu

Part 0 tworzy `FrozenRunProvenanceV1`. Każdy summary i evidence part musi być
exact równy temu anchorowi w zakresie:

- `run_id`, build SHA i clean bit;
- Gatekeeper i brain config hashes;
- rollout i wszystkie schema versions;
- Wire V1 manifest hash;
- profile ID/hash;
- effective-config payload/hash.

Wzajemnie zgodny summary/evidence part 1 z innym provenance jest odrzucany.
`brain_config_hash` jest exact w obrębie runu, lecz pozostaje provenance-only
między różnymi runami bundle; cross-run equivalence używa Gatekeeper, profile i
effective-config hashes.

## 6. BURN contract — wycofany

Pre-run drafty V1/V2 nie identyfikują żadnego prospective row. Artifact V2,
jego manifest binding oraz burn-specific public audit entry point zostały
usunięte. Bundle pozostaje opcjonalnym offline consistency narzędziem i nie
nadaje prospective authority.

UTC buckets są wyprowadzane ze wszystkich poprawnie sparowanych
`paired_decision_timestamp_ms`, a nie wyłącznie z początku runu. Każdy run musi
przejść przed agregacją.

## 7. Typed cutover scope i CI

Raporty single-run i bundle zawierają zamknięty enum:

```text
cutover_scope = metric_contracts_v1_1_profile_a_equivalence_only
```

Pole jest obecne także wtedy, gdy wynik jest fail/not-evaluable, dzięki czemu
consumer nie rekonstruuje scope z tekstu terminal class. `PASS_CUTOVER_READY`
jest dozwolony wyłącznie w tym scope.

Workflow PR2C reaguje również na `PLANS/**`. Release job najpierw listuje testy
i wymaga dokładnie jednego exact match dla harnessu, więc filtr uruchamiający
zero testów nie może zostać uznany za PASS. Harness wykonuje 16 warmup oraz 200
measured iterations w release mode.

## 8. Zakres zamrożony

Bez zmian pozostają:

- Gatekeeper V2/V2.5/V3 thresholds, weights, phases, soft points, verdicts i
  reason codes;
- authority Profile A;
- DecisionLogger v33 schema oraz V3 replay v1;
- selector score, IWIM, sender, Jito, execution i post-buy;
- active dev source i live/shadow boundary;
- rollout `Legacy`.

Nie rozpoczęto PR3 ani Type-5 T1. Nie aktywowano DualCompute ani V2 rollout.

## 9. Weryfikacja i stan decyzji

Focused suites przeszły: durability `17/17`, replay `10/10`, comparator `8/8`
i audit `26/26`. Release harness wykonał 16 warmup i 200 mierzonych iteracji:

```text
full producer-to-final-bytes p50/p95/p99/max = 3545/3545/3545/3545 us
complete snapshot p50/p95/p99 = 1558/1963/2267 us
projection p50/p95/p99 = 476/669/733 us
pair construction p50/p95/p99 = 373/509/630 us
serialization diagnostic p50/p95/p99 = 43/67/79 us
comparator p50/p95/p99 = 5/8/21 us
Wire p95/max = 2339/2339 B
sidecar p95/p99 = 22180/22180 B
v34 p95 = 1176 B
```

Pierwszy workflow amendmentu ujawnił niezależny problem przenośności cache:
repozytoryjne `.cargo/config.toml` ustawia deweloperskie
`target-cpu=native`, a GitHub-hosted cache może zostać odtworzony na innym
wariancie x86_64. Oba joby zatrzymały się przed testami na `rustc SIGILL` przy
ładowaniu cached proc-macro dylib. Workflow PR2C zamraża dlatego CI/resource
baseline na `RUSTFLAGS=-C target-cpu=x86-64` i używa nowych, rozdzielonych
portable cache keys. Nie zmienia to profilu ani zachowania runtime; usuwa
niedeterministyczne powiązanie wyniku CI z CPU poprzedniego runnera.

Semantyczne testy single-run/bundle audit działają w nieoptymalizowanym profilu
i używają deterministycznego, poprawnego histogramu fixture. Dzięki temu
sprawdzają schema/replay/provenance/minima oraz wszystkie mutacje integralności
histogramu, ale nie mylą debugowej szybkości współdzielonego runnera z
acceptance. Jedynym dowodem wydajności pozostaje osobny release harness, który
nie normalizuje telemetryki i mierzy realny ciągły zegar.

Powyższe wyniki pozostają historycznymi diagnostykami. Nie są merge gate'em,
nie wiążą aktywnego BURN contractu i nie nadają prospective authority.

Pełne suites PR2A/PR2B/PR2C, frozen Gatekeeper/lifecycle matrix, replay v1,
checks, targeted Clippy, formatting oraz diff checks były dowodem tego etapu.

## 10. Finalizacyjny amendment 2026-07-14

- osobna bounded queue i osobny PR2C writer task;
- synchroniczny, nieblokujący `try_send`;
- config switch default `false`, efektywnie wyłączony poza dedykowanym shadow;
- raw v33 payload, plane expansion i hydration pozostają na historycznej
  ścieżce v33;
- full PR2C snapshot jest zatrzymywany wyłącznie przy efektywnym ON;
- referencyjny `serde_json_canonicalizer`, bez własnego JCS i fixed-width JSON
  mutation;
- bounded shutdown i niezależny completion proof po directory sync;
- `BURN_IN_CONTRACT_V2` oraz burn-specific audit entry point usunięte;
- latency p50/p95/p99/max jest diagnostyką, nie acceptance gate'em;
- prospective burn-in i PR3 pozostają nieautoryzowane/nieuruchomione.
