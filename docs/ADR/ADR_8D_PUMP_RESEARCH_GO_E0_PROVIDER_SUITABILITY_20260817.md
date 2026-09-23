# ADR-8D: Pump Research GO-E0 — bounded provider suitability bez exact outputu

**Data:** 2026-08-17

**Status:** IMPLEMENTED / EXECUTED ONCE / BLOCKED FAIL-CLOSED

**Task:** `PUMP_RESEARCH_GO_E0_PROVIDER_SUITABILITY`

## D0. Problem

Pełny independent source-completeness audit obejmuje około 4,3 tysiąca
finalized slotów. Przy concurrency `1`, dwóch retry i timeout `30 s`
niezweryfikowany provider mógł zużyć wiele godzin, a następnie pozostawić duży
lecz niekwalifikowalny exact artifact. Lokalny review configu dowodził jedynie
poprawnej składni, oddzielnego provider ID/hostname i no-legacy auth; nie
dowodził retention, pełnego `getBlock`, inner instructions, loaded addresses,
failed transactions, tx order ani stabilnej przepustowości.

Wcześniejszy preparation snapshot pozostawał poprawnie w stanie
`HOLD_PROVIDER_IO_AND_CERTIFY`. Potrzebna była osobna, bounded i read-only
bramka przed combined audit, bez mutacji raw i bez utworzenia exact outputu.

## D1. Decyzja: pomocnicza operacja `provider-suitability`

Standalone binary otrzymało jawny subcommand:

```text
pump-research-tape provider-suitability \
  --run-dir <closed-raw-dir> \
  --qualification-audit-config <protected-config> \
  --preparation-receipt <qualification-preparation-receipt> \
  --expected-preparation-sha256 <sha256> \
  --output <new-operator-log-dir>
```

Jest to wyłącznie qualification-preparation control plane. Operacja nie
wywołuje certifiera ani exact writera. Najpierw przechodzi pełny frozen-V1 raw
index, a następnie wybiera ograniczony zestaw slotów:

- first, midpoint i last wyznaczonego qualification range;
- 16 równomiernie rozłożonych burst slots;
- reprezentatywne sloty direct top-level, inner CPI, router-to-Pump CPI,
  v0 loaded address i failed Pump transaction.

Raw representative scan jest ograniczony do 250 000 transakcji. RPC działa
sekwencyjnie, ma trzy kolejne unavailable jako circuit breaker i provider wall
budget `420 000 ms`. Każda próbka używa dokładnie tego samego klienta i
`RpcBlockConfig` co pełny audit:

```text
encoding = Json
transaction_details = Full
rewards = false
commitment = finalized
max_supported_transaction_version = 0
```

Porównanie obejmuje identity multiset `(slot, tx_index, signature)`, wszystkie
cztery invocation-class counts oraz failed-status multiset. Brak meta, inner
instructions albo loaded addresses dla v0 jest typed provider unavailability,
nie imputacją.

## D2. Fail-closed granice i provenance

GO-E0:

- parsuje i hashuje te same, jednokrotnie odczytane bytes audit configu;
- wiąże expected preparation SHA, audit config, exact executable oraz trzy raw
  control artifacts;
- ponownie sprawdza ich digesty po provider phase;
- przechwytuje dokładny token użyty przez klienta i skanuje nim receipt przed
  publikacją;
- redaguje literalny endpoint i hostname z provider errors;
- nie pozwala umieścić outputu w raw subtree ani w/poniżej planowanego exact;
- wymaga nowego output path i publikuje receipt przez `.partial`, file sync,
  directory sync i rename;
- zapisuje wyłącznie `raw_write_attempt_count = 0`,
  `exact_output_created = false`, `certify_started = false`,
  `export_started = false` i `strategy_started = false`;
- zwraca non-zero dla każdego statusu innego niż `ReadyForFullAudit`, ale
  zachowuje typed receipt.

Output path TOCTOU i config/hash TOCTOU zostały znalezione podczas
przed-provider review i naprawione przed pierwszym RPC. Regresja potwierdza,
że operator-log output jest dozwolony, a raw/exact path failuje przed I/O.

## D3. Jedyny wykonany probe

GO-E0 uruchomiono raz dla:

```text
run_id = pump-research-1786909252793-3799414
```

Control evidence przed startem:

```text
preparation SHA-256 = eab36576a3ad3284fe73da186186f04301a6b5a0809b2e592cf72ca3c7dd0787
audit config SHA-256 = c5e1ebb6585639ebe33c70308a838e102d00aa5f45a46012b581e0cb56d9ca16
GO-E0 executable SHA-256 = 7b22ac420b65f7d710adc33b69bde75ed96588e4580185212a06769556308be6
```

Finalny receipt:

```text
datasets/pump-research/operator-logs/
  go-e0-provider-suitability-v1-20260817T062928Z/
  provider_suitability_receipt_v1.json

SHA-256 = 783723443ad47c7e2ae1e9f4d04ac08e37848ea750fd803814de48b6e29fb910
mode = 0600
status = blocked_audit_unavailable
process exit = 1
```

Wynik:

```text
range                         439703807..=439708174
sample / attempted            19 / 19
matched                       16
unavailable                   1
request attempts              22
provider elapsed              136 939 ms
representative raw examined   5
missing representative roles  0
```

Jeden slot wyczerpał trzy próby po 30 sekund; inny zwrócił block dopiero po
retry w około 32 sekundy. Techniczna dostępność/capacity dla pełnego zakresu
nie została więc udowodniona.

Probe wykrył także wcześniejszy lower-bound defect materializera. Pierwsze dwa
sloty qualification range były niepełne po stronie capture:

```text
439703807  raw=0  audit=118
439703837  raw=3  audit=90
439703838  raw=85 audit=85  router+CPI+v0 exact match
```

Obecne `first rooted + 1` nie stanowi dowodu pierwszego kompletnego slotu,
gdy stream został zestawiony później albo w środku slotu. Combined audit musi
pozostać HOLD do wprowadzenia zachowanej first-complete-block/stream boundary
i regresji mid-slot capture start. Findings nie mogą zostać usunięte ani
przeklasyfikowane na błąd providera.

Receipt zachowuje
`provider_identity_independence_verified = false`. Różny provider ID/hostname
nie jest fizycznym dowodem niezależności operatora.

## D4. Weryfikacja, wpływ i następna bramka

Przed GO-E0 przeszły:

- `cargo fmt --all -- --check`;
- `cargo check -p seer --lib`;
- `cargo check -p seer --bin pump-research-tape`;
- `research_tape`: 53 passed, 1 ignored;
- `research_tape_materializer`: 17 passed;
- CLI: 7 passed;
- `grpc_connection::tests`: 95 passed;
- `rpc_http_client`: 6 passed;
- frozen `ghost-core::pump_research_tape`: 10 passed;
- CS0: 2 passed;
- parser parity: 1 passed;
- future-capture supervisor: 10 passed;
- release capture-enabled harness: 1 passed, `8192/8192/8192`, zero drops/gaps,
  one segment, clean writer, p99 `182 ns` przy SLA `100 000 ns`.

GO-E0 ponownie wykonał pełny raw index, w tym frame/footer/chain oraz whole-file
SHA-256/BLAKE3 validation. Po probe nadal istnieje 25 segmentów, zero
`.partial`, zero exact directories i brak aktywnego procesu. Literalny audit
endpoint i hostname nie występują w receipcie.

Zmiana nie dotyka frozen raw V1, Yellowstone requestu, active Seer configu,
Event Busa, OracleRuntime, Gatekeepera, MFS ani execution. Rollback oznacza
niewykonywanie `provider-suitability`; opublikowany blocked receipt pozostaje
forensic evidence i nie autoryzuje retry.

Następna dopuszczalna praca jest lokalną korektą qualification start boundary
oraz osobną decyzją o provider capacity/config. Combined certify, exact output,
export-window, strategia, Gatekeeper i execution pozostają HOLD/NO-GO.
