# PR1E 1E-B — production authority cutover qualification receipt

Status:
`IMPLEMENTED / OFFLINE AND DIFFERENTIAL GATES PASS / CLOSED RUNTIME RUN PENDING / DRAFT PR`

Base: `103212b16bfc059db367e1ceb3c7d00fd307d6c5`

Commit 1E-A:
`6f08297ca5d362fb593f8d53e0c9710f79f3751e`

Gałąź:
`agent/ingest-state-quote-boundary-pr1e-20260727`

## 1. Zakwalifikowany kontrakt

1E-B aktywuje jeden runtime authority dla nowych strukturalnych mutacji:

```text
primary raw wrapper + aligned ObservedPumpMutationV1
→ production PumpObservationLedgerV1
→ private CanonicalRuntimePermitV1
→ existing rich wrapper
→ Event Bus
→ actual downstream apply
→ CandidateIntegrity Ready/CAS
→ MFS
→ Gatekeeper
→ guarded submit
```

Nie istnieje produkcyjny per-event fallback do parent wrappera. Missing
observation, mismatch, brak canonical mutation, unavailable Ledger/registry,
capacity failure albo receipt ambiguity zamykają admission nowych kandydatów.
Secondary raw, parsed NLN, exact duplicate, `ContinuityOnly` i `Suppressed`
mają zero strukturalnych runtime emissions.

## 2. Zamrożone corpusy i runner

Digesty istniejących corpusów pozostały niezmienione:

| Kontrakt | BLAKE3 |
|---|---|
| PR1B legacy parser projection | `549d66a347a3e56b516bc5b77a5f22929604442d409ece7eb1a55525eaa51202` |
| PR1C AccountObservationArbiter v2 | `63839d047310638fe0d8643ee6c71148ac292f4390fc9098a2e573ce0ac1e051` |
| PR1D PumpObservationLedger v1 | `833de2bd384c964712f2e7127f9bc1db57745644633c1c66facef540cdf4c2a4` |
| PR1D PumpObservationLedger v2 | `c81d7b4f0cc3792c2bb2c4e71bfd0634fcfdd69723758d741ee2405770603415` |
| PR1D full parser snapshot v2 | `507b13704d5b90c3f724a395acbf0d0cc55fdc37a83fcb95cf67cceb6247569f` |
| PR1E manifest | `cd28c798082999cf2377842199ffabb6601f7115417da244a3cf864e5ef27208` |
| PR1E cross-layer corpus | `30fbf78344afd77958fe573af5c2414139023db4c770b03c0f710026b7cdd38c` |

Finalna wersja wykonywalnego runnera:

```text
ghost-launcher/src/pr1e_qualification.rs
SHA-256 = 1cb7b1a9e91a0f3cdd13ba2ba6e208d96a3ad2650a4a55643c24060bd549d2f2
```

Runner używa produkcyjnych:

- `PumpObservationLedgerV1`;
- `CandidateIntegrityRegistry`;
- launcherowego typed admission;
- `SessionPoolTradeBridge`;
- Event Bus adaptera;
- exact downstream apply receipt.

Fake Ledger: `0`. Fake CandidateIntegrity: `0`.

## 3. Targeted correctness i fault injection

| Bramka | Wynik |
|---|---|
| `cargo test -p ghost-launcher --lib pr1e_ -- --nocapture` | PASS — 15/15 |
| `cargo test -p ghost-launcher --lib candidate_integrity::tests:: -- --nocapture` | PASS — 17/17 |
| AccountObservationArbiter frozen corpus | PASS — 2/2 |
| PumpObservationLedger frozen V1/V2 corpus | PASS — 3/3 |
| PumpObservationLedger adversarial/fault corpus | PASS — 40/40 |
| PumpObservationLedger targeted unit contract | PASS — 9/9 |
| PR1B legacy parser digest | PASS |
| PR1D full parser digest | PASS |

Testy PR1E obejmują między innymi:

- missing primary observation i wrapper mismatch;
- mixed provider role/ID;
- exact duplicate, secondary raw i parsed witness suppression;
- ledger/registry capacity exhaustion i poisoned Ledger;
- incomplete inventory i per-candidate exact locator isolation;
- Event Bus delivery bez downstream apply;
- duplicate/ignored/terminal/failed apply;
- buffered permit replay, expiry i eviction;
- active MFS/evaluation guard;
- submit-before-conflict, post-submit reconciliation i confirmed-position
  quarantine;
- startup bez Event Bus/registry oraz legacy source;
- `ContinuityOnly` z zerem strukturalnych emisji;
- duplicate AccountUpdate i provider conflict.

## 4. Package/workspace gate matrix

| Polecenie | Wynik | Receipt SHA-256 |
|---|---|---|
| `cargo fmt --all --check` | PASS | pusty log |
| `git diff --check` | PASS | pusty log |
| `cargo test -p ghost-core` | EXPECTED_BASELINE_FAILURE | `31013cbe989815a435ec1d95ff5e390c74ed641c31703e23b24baedf0a259957` |
| `timeout 300s cargo test -p seer --no-fail-fast` | EXPECTED_BASELINE_FAILURE | `4d203ea4297635d0babb8b48a9b02d075df26dc062a5ac0a35b5d7a50f7d59aa` |
| `cargo test -p trigger` | EXPECTED_BASELINE_FAILURE | `1cd2b0250b67f58c5cc98b13e788e310823eb49539ef1f1ee5702e27a3ceac71` |
| `cargo test -p ghost-launcher` | EXPECTED_BASELINE_FAILURE | `bccf88cd8977cb515855f42d506cfe5d5307adcdbca665739a92197c7dea8709` |
| `cargo test --workspace --no-fail-fast` | EXPECTED_BASELINE_FAILURE | `131ff3a060ef6cb75753f959f61c281a74d2111d07b9b485431ba8bc7def46fb` |
| `cargo build --release --workspace` | PASS — 608 s | `368a7777879abea8b431c9a8f3e6795218b3d9edacb4167426d4418ea3826783` |

Zamrożone czerwone klasy pozostały bez zmian:

- `ghost-core`: wyłącznie `InvalidTagEncoding(104)`;
- Seer: dokładnie 12 historycznych failure testów biblioteki i jeden
  `source_router`;
- trigger: dwa brakujące `status_uuid` oraz
  `presigned.size_bytes < 700`;
- launcher/workspace: wyłącznie testowe `PoolTransaction E0063` z tym samym
  missing-field setem. Każdy ujawniony target istniał już na
  `103212b` i PR1E go nie zmienił.

Nie naprawiono żadnego odziedziczonego fixture w PR1E.

## 5. Formalny parent-versus-PR1E performance protocol

Warunki:

- parent: `103212b16bfc059db367e1ceb3c7d00fd307d6c5`;
- current: finalny frozen diff PR1E;
- osobne release test-binary;
- ten sam host, profil i workload;
- 5 warm-upów na wariant;
- 20 naprzemiennych paired rounds;
- 100 000 bootstrap samples, seed `20260726`;
- brak równoległego Cargo lub repo workload.

| Bramka | Wynik |
|---|---:|
| throughput geometric mean ratio | `1.026863643` |
| throughput lower one-sided 95% CI | `1.018097198` — PASS (`>= 0.98`) |
| receive-to-normalize p99 geometric mean ratio | `0.913547337` |
| receive-to-normalize p99 upper one-sided 95% CI | `0.958477979` — PASS (`<= 1.05`) |
| missing events | `0` we wszystkich 50 runach |
| silent drops | `0` we wszystkich 50 runach |
| saturation receiver blocked | `0` |
| saturation blocking wait | `0 ns` |
| slow IPC receiver blocking waits | `0` |
| parser V1/V2 digest drift | `0` |

Synthetic slow-WAL writer wykonał oczekiwane opóźnienie we własnym workerze.
Receiver enqueue pozostał nieblokujący względem writer delay: maksimum
`1.232519 ms`, przy minimalnym writer completion `6.964297 ms`.

Pozostałe opisowe mediany/zakresy oraz wszystkie pary znajdują się w
`pairs.tsv` i `summary.json`.

Raw receipts:

| Artefakt | SHA-256 |
|---|---|
| `/tmp/pr1e_perf_protocol_20260727_v1/metadata.json` | `f0e1bfe074b43918a0c75f6aab74a4cbf180db55d998b0059ceeebe019ea4684` |
| `/tmp/pr1e_perf_protocol_20260727_v1/runs.jsonl` | `3105cfa8193978ca295e00e7a7ac664ccd8af13eda1a3de9b7330479a5a6adc6` |
| `/tmp/pr1e_perf_protocol_20260727_v1/pairs.tsv` | `597c583819dd439fe9980ff43a42faf4bcf868148d6bed0ca7906327d29c25ea` |
| `/tmp/pr1e_perf_protocol_20260727_v1/summary.json` | `8e18d9564154eadee2b2d6e977acbb9b62216d892fe7fe42b29ac8bb981e363b` |
| `/tmp/pr1e_perf_protocol_20260727_v1/raw_receipts.sha256` | `e756c351e8d14ff6218f300cfbd8bddd2067abaef3ea68f29500b8b3b69d32b3` |

Performance result: `PASS`. Waiver PR1D nie został użyty.

## 6. Rollback identity

```text
previous baseline binary inventory hash:
e0ee4230901002efdcdd9e4d26237d3973a24f59f714cf1a1e74d37738747d27

previous config SHA-256:
eecc6462eaa98325a899bc4de19fb5fba7387f74ac96e3c994126379fb60e737

final PR1E release binary SHA-256:
a916d87088119683bda64462f7bc6defa34f2cdee9aa26068a8ff84afa709540

final PR1E config SHA-256:
eecc6462eaa98325a899bc4de19fb5fba7387f74ac96e3c994126379fb60e737
```

Rollback jest wyłącznie atomowym przywróceniem poprzedniego binary i configu.
Nie istnieje runtime `Legacy`, produkcyjny `Observe` ani per-event fallback.

## 7. Zamknięty runtime qualification run

Gate 30 min / minimum 10 000 successful primary raw Pump mutations nie został
sfabrykowany ani zastąpiony testem. W środowisku implementacyjnym brak:

```text
GHOST_SEER_GRPC_ENDPOINT
GHOST_SEER_GRPC_X_TOKEN
```

`config.toml` pozostaje poprawnie `shadow_only` / `shadow`, ale bez endpointu
i tokenu nie można uruchomić production-like Yellowstone inputu. W związku z
tym:

- Draft PR może zostać otwarty do review;
- PR1E nie jest jeszcze merge-qualified;
- przed merge należy uruchomić finalny release binary na production-like
  configu, zapisać authority epoch receipt i wykazać minimum 30 min oraz
  10 000 successful primary mutations;
- wynik musi spełnić wszystkie gate'y sekcji 19 planu.

Status tego gate'u: `EXTERNAL ENVIRONMENT BLOCKED / REQUIRED BEFORE MERGE`.

## 8. Wniosek

Offline correctness, differential, fault-injection, package comparison,
release i formalny performance protocol są zakończone. Cutover nie dotyka
ekonomiki, quote math, strategii, MFS schema ani shadow/live configu.

Draft PR pozostaje świadomie niegotowy do merge wyłącznie do czasu
zamkniętego runtime qualification runu i receipt authority epoch.
