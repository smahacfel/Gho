# ADR-8D: Shadow V2 L2 Offline Research-Grade Execution Plan 20260704

## Status

Accepted as execution plan artifact.

## Decision

Zapisujemy zaakceptowany plan dojscia do Shadow Burnin V2 L2 jako artefakt w:

```text
PLANS/DO_REALIZACJI/PLAN_SHADOW_V2_L2_OFFLINE_RESEARCH_GRADE_CANDIDATE_20260704.md
```

Docelowy verdict planowanej sekwencji:

```text
SHADOW_V2_L2_RESEARCH_GRADE_CANDIDATE_OFFLINE_ONLY
```

Ten ADR nie przyznaje runtime approval, live-equivalence, strategy unlock,
active close ani `shadow_close_only`.

## Context

PR47 / PR43-E zostal przyjety jako partial source propagation stage. Po tym
etapie Shadow V2 ma sensowna powierzchnie provenance/order, ale nadal nie ma
L2. Aktualne blokery L2 sa:

1. `account_data_hash` z raw account bytes;
2. realny temporal/source proof bez fake source joins;
3. density / horizon / retention contract;
4. Gatekeeper denominator / starvation audit;
5. dedicated research validation sample.

Wazne rozroznienie architektoniczne:

```text
L1 = executable diagnostic simulation
L2 = offline research-grade candidate
L3 = live-equivalence
```

## Scope

Plan dzieli dalsza prace na stage-labels, a nie historyczne numery GitHub PR:

```text
L2-A0: account_data_hash source audit
L2-B: account_data_hash propagation
L2-C: temporal source proof closure
L2-D: density / horizon contract
L2-E: Gatekeeper denominator coverage
L2-F: dedicated research validation run
```

Nazwy `L2-A0` ... `L2-F` maja zapobiec pomylkom z istniejacymi juz PR44,
PR45, PR46 i PR47.

## Invariants

Plan zachowuje nastepujace granice:

- nie zmienia BUY/REJECT;
- nie zmienia Gatekeeper policy;
- nie zmienia selector runtime;
- nie dotyka TX/Jito/live path;
- nie dotyka R51;
- nie wlacza active close;
- nie wlacza `shadow_close_only`;
- nie promuje Shadow V2 evidence do decision input;
- nie luzuje `has_complete_chain_order()`;
- nie miesza transaction-order proof z account-state proof;
- nie traktuje Solana-native `log_index` jako brakujacego pola providera.

## Key Design Decisions

### Hash provenance

`account_data_hash` ma byc liczony wylacznie jako:

```text
BLAKE3(original raw account update bytes)
```

Nie wolno liczyc hashy z decoded struct, reserves, JSON, `CanonicalPoolState`
ani innych runtime-derived reprezentacji.

### Raw bytes retention

Raw bytes nie powinny byc przenoszone dalej niz jest to konieczne. Domyslny
model:

```text
raw bytes at ingest -> BLAKE3 close to ingest -> carry hash + metadata
```

Do dalszych warstw powinny isc hash i metadata, nie raw account bytes.

### Transaction proof vs account-state proof

Dla transaction-like evidence obowiazuje transaction source proof:

```text
slot/block_time + source_tx_signature + tx_index + instruction_index
```

Dla account-state pool-state samples obowiazuje account-state proof:

```text
account_pubkey + slot + write_version + BLAKE3(raw account bytes)
```

Te proofy sa rozne:

```text
transaction_source_proof_complete != account_state_source_proof_complete
```

`has_complete_chain_order()` nie moze zostac poluzowany tak, zeby account-state
proof udawal transaction-order proof.

### Horizon baseline

Pierwszy L2 baseline obejmuje tylko jawnie zadeklarowane horyzonty:

```text
2000
3000
10000
30000
120000
```

Horyzonty `300000` i `500000` pozostaja:

```text
NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
```

Nie moga blokowac pierwszego L2 baseline, jesli nie zostaly jawnie
zadeklarowane.

### Hash coverage denominator

`account_data_hash_required_coverage = 100%` dotyczy tylko:

```text
observed account-state boundary samples in L2 scope
```

Z denominatora wyklucza sie:

- derived rows;
- not-applicable rows;
- legacy/backward-compatible rows, ktore sa raportowane osobno.

## Consequences

1. Kolejny poprawny ruch to L2-A0 audit-only, a nie implementacja na slepo.
2. L2-B moze isc dopiero po potwierdzeniu raw bytes source.
3. L2-C musi dodac Solana-aware proof semantics bez luzowania istniejacego
   transaction-order predicate.
4. L2-D musi przestac blokowac baseline przez niezadeklarowane long horizons.
5. L2-E moze wykryc Gatekeeper starvation, ale nie moze w tym samym PR luzowac
   progow.
6. L2-F jest dedicated research validation run, nie smoke.

## Rejected Alternatives

### Uzyc starych nazw PR44/PR45/PR46

Odrzucone. Repo jest juz po PR47, wiec historyczne numery jako nazwy nowych
stage'y zwiekszaja ryzyko pomylek.

### Liczyc hash z decoded state

Odrzucone. Hash decoded struct nie dowodzi chain-observed raw account state.

### Wymagac transaction signature dla account update pool-state proof

Odrzucone. Account update nie musi byc transaction-like eventem. Wlasciwy proof
dla pool-state account update to account-state proof.

### Luzowac has_complete_chain_order()

Odrzucone. L2 ma dostac osobna Solana-aware audit classification, a nie
rozwodniony transaction-order predicate.

### Blokowac pierwszy L2 baseline przez 300s/500s

Odrzucone, dopoki te horyzonty nie zostana jawnie zadeklarowane dla danego runu.

## Verification

Dla samego zapisu planu wystarcza:

```bash
git diff --check
git status --short
```

Dla kolejnych implementacyjnych stage'y plan wymaga osobnych cargo/python
guardow opisanych w pliku planu.

## Final Decision

```text
plan_artifact=PLANS/DO_REALIZACJI/PLAN_SHADOW_V2_L2_OFFLINE_RESEARCH_GRADE_CANDIDATE_20260704.md
final_target=SHADOW_V2_L2_RESEARCH_GRADE_CANDIDATE_OFFLINE_ONLY
runtime_approval=false
live_equivalence=false
strategy_research_unblocked=false
recommended_next_stage=L2-A0_ACCOUNT_DATA_HASH_SOURCE_AUDIT
```
