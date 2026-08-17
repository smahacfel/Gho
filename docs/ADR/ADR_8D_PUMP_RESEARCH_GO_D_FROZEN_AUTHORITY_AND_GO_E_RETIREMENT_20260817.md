# ADR-8D: Pump Research — GO-D frozen authority and GO-E retirement

**Data:** 2026-08-17

**Status:** IMPLEMENTED / LOCALLY VERIFIED / GO-D VERIFIED / GO-E RETIRED

**Task:** `PUMP_RESEARCH_GO_D_FROZEN_AUTHORITY_AND_GO_E_RETIREMENT`

## D0. Problem

GO-D raw tape przeszedł sealed provenance v5, clean lifecycle, pełny frozen-V1
scan oraz niezależne SHA-256/BLAKE3 wszystkich 25 segmentów. Zewnętrzny GO-E
zależał natomiast od jakości, retencji i limitów niezależnego RPC. Combined
audit zakończył się `Blocked(SourceCoverageUnproven)` po brakach providera,
mimo że nie wykrył naruszenia frozen kontraktu GO-D.

Traktowanie niedostępności RPC jako gate’a dla historycznego surowca mieszało
dwie odrębne authority: integralność własnego zamrożonego tape oraz dostępność
zewnętrznej usługi.

## D1. Decyzja

Obowiązuje:

```text
GO_D_SOURCE_AUTHORITY = VERIFIED
GO_E_EXTERNAL_AUDIT = RETIRED / NOT A GATE
```

GO-D jest jedyną source authority dla przypisanych mu offline eksperymentów.
HTTP 503, pruning, rate limit, timeout albo inna niedostępność GO-E nie może
blokować, unieważniać ani opóźniać pracy na GO-D.

GO-E artifacts pozostają historycznym materiałem forensic. Nie są wejściem do
statusu, exportu, prerejestracji lub analizy strategii.

## D2. Hash-pinned GO-D authority

Promocja nie jest automatyczna dla dowolnego raw runu. Wymaga create-new:

```text
configs/rollout/pump-research-go-d-source-authority-v1.json
SHA-256 = b583dd1a6a24a87c2035837e3ef0dc9266a35041505fd07f8095987ab1088ab7
```

Receipt wiąże:

- `source_run_id = pump-research-1786909252793-3799414`;
- storage format V1;
- operator preflight binding SHA-256;
- start manifest SHA-256;
- completion receipt SHA-256;
- aggregate pinned raw segment-set BLAKE3;
- literalne `VERIFIED` i `RETIRED_NOT_A_GATE`.

## D3. Offline materialization

Nowy tryb CLI:

```text
certify
--run-dir <GO-D/raw>
--output <create-new exact>
--go-d-source-authority <receipt>
--expected-go-d-source-authority-sha256 <hex>
```

Operacja:

1. nie otwiera RPC ani Yellowstone;
2. weryfikuje output/raw disjoint przed side effect;
3. wykonuje pełny frozen-V1 index;
4. kopiuje segmenty do anonymous `O_TMPFILE` snapshot FD;
5. wiąże receipt z control files i aggregate segment-set digest;
6. materializuje wyłącznie z pinned descriptors;
7. ponownie sprawdza authority i raw bezpośrednio przed finalnym rename.

Plain `certify` pozostaje development-only `Unqualified`.

## D4. Typed status i raporty

Dodano addytywny status:

```text
PumpResearchTapeQualificationStatusV1::VerifiedFrozenTape
```

Exact manifest i authority report zapisują:

```text
GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = true
go_d_source_authority_sha256 = <exact receipt SHA-256>
```

Historical `Ready` pozostaje czytelne dla kompatybilności, ale nie jest już
dopuszczonym gate’em `export-window`. Export wymaga `VerifiedFrozenTape` i
wszystkich trzech GO-D authority bindings.

## D5. Fail-closed granice

GO-D authority nie może nadpisać:

- legacy/unsupported capture provenance;
- ProgramData version boundary;
- incomplete lub nieclean lifecycle;
- `received != admitted != persisted`;
- dropped update, coverage gap lub missing ingress event;
- driftu binding/start/completion receiptu;
- driftu source authority receiptu;
- driftu segment paths lub pinned snapshot FD;
- output znajdującego się wewnątrz raw.

Każdy z tych przypadków blokuje final exact publication.

## D6. GO-E retirement w CLI

`provider-suitability` i audit-backed `certify` kończą się lokalnym błędem
przed provider I/O. Kod historycznych readerów pozostaje dostępny do odczytu
już opublikowanych receiptów i do regresji, ale nie jest promotion path.

Nieudany output `exact-go-e-spectrum-v1` pozostaje historyczny
`Blocked(SourceCoverageUnproven)` i nie jest nadpisywany ani usuwany.

## D7. Eksperymenty

`VerifiedFrozenTape` nie otwiera outcome’ów automatycznie. Każdy eksperyment
nadal musi przed outcome’ami zamrozić:

- hipotezę i universe;
- decision cutoff;
- SELECTED/REST i outcome-blind exclusion codes;
- entry/exit semantics;
- metryki i power gate;
- dokładne file hashes, newline-complete offsets oraz SHA helpera.

Nie wolno używać RPC backfillu, imputacji, carry-forward, nowego capture’u ani
zmiany runtime’u do naprawiania historycznego eksperymentu.

## D8. Zakres wpływu

Zmiana dotyczy wyłącznie frozen storage metadata, offline materializera, CLI,
export authority, planu i dokumentacji. Nie zmienia:

- bytes lub formatu frozen raw V1;
- GO-D raw/control artifacts;
- capture subscription/writera;
- aktywnego Seera i OracleRuntime;
- `MaterializedFeatureSet`;
- Gatekeepera;
- strategii lub execution.

## D9. Weryfikacja

Przed publikacją PR przeszły:

```text
verified GO-D public materialization regression              PASS
stale authority / segment-set rejection                      PASS
historical exact-manifest JSON compatibility                 PASS
export rejects historical Ready and accepts VerifiedFrozenTape PASS
research_tape_materializer                                   49 passed
research_tape                                                86 passed, 1 ignored
frozen ghost-core Pump Research                              11 passed
CS0                                                           2 passed
parser parity                                                 1 passed
CLI corpus                                                    6 passed
capture supervisor                                           10 passed
locked release build                                         PASS
release capture-enabled harness                              PASS
cargo fmt/check                                               PASS
clean staged-diff check in isolated worktree                 PASS
```

Release harness zachował `8192 = received = admitted = accepted`, zero dropów,
gapów i persisted missing events, jeden opublikowany segment, clean writer,
brak capture abort oraz receive hand-off p99 `210 ns` przy limicie `100 000
ns`. Istniejące warningi szerokiego checkoutu nie zostały rozszerzone ani
naprawiane w tym PR.
