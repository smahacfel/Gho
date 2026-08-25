# ADR-8D: Prospective Pump Exact-State Tape V2 — stream-only V1.1 readiness without account snapshot

**Data:** 2026-08-23

**Status:** IMPLEMENTED LOCALLY / FINAL LOCAL MATRIX PASS / FINAL NEUTRAL SELF-REVIEW PASS / FRESH INDEPENDENT REVIEW PASS (P0=0, P1=0, P2=0) / NO PROVIDER I/O / NO RAW V3 CREATED

**Typ:** ADR-8D / prospective research evidence / fail-closed source-contract revision

> Globalny szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie był dostępny ani w
> tym checkoutie, ani pod wskazaną ścieżką. Dokument zachowuje lokalny układ
> ADR-8D używany przez wcześniejsze ADR-y V2.

## D0. Decyzja i granica

Wcześniejszy kontrakt prospective V2 próbował uzyskać pełne Pump account
universe przez all-owner Yellowstone subscription oraz bootstrap
`getProgramAccounts(Pump)`. Jest to niewłaściwe dla bieżącej iteracji: globalny
scan jest wielogigabajtową, niepaginowaną odpowiedzią RPC i nie jest konieczny
do przyszłej, stream-only kwalifikacji exact state.

PRXTAPE3 zastępuje ten fragment jednym, prospektywnym kontraktem V1.1:

```text
Yellowstone Pump transactions
+ Yellowstone canonical BondingCurve / Global account updates
+ Yellowstone Slot + BlockMeta + unfiltered full blocks
+ ProgramData start/end receipts only
= prospective, fail-closed raw evidence
```

Nie ma:

- `getProgramAccounts`, filtrowanego GPA, `getMultipleAccounts`, paginacji,
  snapshot exportu ani RPC account backfillu;
- historycznego baseline'u stanów kont;
- `OtherPumpOwned` w physical raw contract;
- migracji lub kwalifikacji PRXTAPE2.

RPC ma tylko mały, zamknięty cel: start/end receipts dla Pump Program i
ProgramData. V1/GO-D, GO-E oraz aktywny Seer/Gatekeeper/execution pozostają
poza zakresem.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```

## D1. PRXTAPE3 source contract

Physical raw contract przed pierwszą prawidłową taśmą zostaje jawnie
zrewidowany:

```text
raw storage revision       = 3
segment magic              = PRXTAPE3
capture config schema      = 3
run manifest/receipt       = 3
operator preflight schema  = 4
capability receipt schema  = 3
source capture semantics   = decoded_protobuf_schema_lossless_bonding_curve_global_and_full_blocks_v4
```

Pump IDL, semantics schema oraz exact JSONL/window schemas nie zmieniają
kształtu. Raw enum PRXTAPE3 zawiera source projections, lossless retained
protobuf payloads, full-block start/chunk/completion, coverage gaps, segment
footer i dokładnie jeden `ProspectiveStreamBoundary`; nie zawiera żadnego
bootstrap/snapshot recordu.

SubscribeRequest V2 ma dosłownie dwa account filters:

1. owner Pump + BondingCurve discriminator;
2. exact canonical Pump Global pubkey.

Account update niezgodny z jedną z tych klas jest błędem source contractu,
który kończy run `Incomplete`; nie jest przechowywany jako rozszerzający scope
raw record. Niefiltrowane full blocks pozostają wyłącznie bounded prospective
evidence do literalnej reconciliation:

```text
full-block Pump inventory ↔ filtered Pump transaction lane
```

Nie są snapshotem kont, historią ani substytutem GO-E.

## D2. Jedna durable stream-readiness boundary

Po ustanowieniu Yellowstone recorder czeka najwyżej
`source_readiness_timeout_ms` na pierwszy **trwale zaakceptowany** rekord
każdego z pięciu lane'ów:

```text
Pump transaction
BondingCurve/Global account update
Slot
BlockMeta
full block
```

Następnie zapisuje dokładnie jeden record:

```rust
PumpExactStateProspectiveStreamBoundaryV2 {
    source_readiness,
    source_stream_epoch,
    source_capture_sequence_exclusive,
    cohort_slots_strictly_after,
    sealed_wall_ts_ms,
    sealed_monotonic_ts_ms,
}
```

Kontrakt boundary jest literalny:

```text
source_readiness_slot          = max(first slots pięciu lane'ów)
cohort_slots_strictly_after    = source_readiness_slot
source epoch                   = niezmieniony
source records przed exclusive = zachowane jako warm-up evidence
boundary                       = dokładnie jedna
writer write + flush + sync    = potwierdzone przed startem cohort timer
```

Writer rezerwuje dla boundary jej własny wewnętrzny marker kolejności. Nie
jest on Yellowstone source recordem ani nie wchodzi do source census; dzięki
temu żaden source record po `source_capture_sequence_exclusive` nie może
zostać fizycznie zapisany przed boundary, nawet gdy control lane dociera do
writer'a po data lane. Offline validator ponownie sprawdza pełny warm-up
prefix, marker i ciągłość kolejnych source sequence values.

Brak lane'u, druga boundary, zmiana epoch, coverage gap albo niedomknięty
full block nie może dać `Complete`. Completion receipt wiąże `source_readiness`,
`readiness_boundary_persisted`, `cohort_slots_strictly_after` i
`readiness_completed`, a writer census ma dokładnie jeden accepted boundary.

## D3. Offline exactness bez baseline'u

Qualifier buduje anchors wyłącznie z canonical, rooted Yellowstone account
updates oraz ignoruje warm-up dla capability: do cohort admission należą tylko
successful rooted mutations ze slotów:

```text
slot > cohort_slots_strictly_after
```

Exact Create/CreateV2 wymaga same-signature final BondingCurve anchor; ten
anchor staje się predecessor dla późniejszych trade'ów curve. Exact trade
wymaga jednoznacznego wcześniej streamed curve anchor i same-signature final
anchor. Pierwszy zaobserwowany trade istniejącej curve bez predecessora jest
literalnym `missing_exact_pre_anchor`, bez RPC, imputacji lub heurystyki.

Denominator pozostaje niepomniejszony:

```text
successful rooted mutations
= exact rooted mutations
+ explicitly non-exact rooted mutations
```

Nie ogranicza się go do nowych launchy. Obowiązują nadal full-block/filter
reconciliation, parent-linked frontier, occurrence conservation, denominator
reconciliation i coverage `>= 999000 ppm`.

Przed `Qualified` run musi również osiągnąć minimalny prospective evidence
size:

```text
cohort elapsed >= 1_800_000 ms
OR
successful rooted mutation denominator >= 10_000
```

Niespełnienie jest typed blockerem `QualificationRunBelowMinimum`. Exporter
udostępnia wyłącznie launch po boundary, dla którego 150 s observation + 90 s
forward mieści się w reconciled full-block frontier i wszystkie observation
curve candidates są exact. `Blocked` ani unscoped candidate nie publikują
finalnego lub `.partial` outputu.

## D4. Wymagane regresje i granice operacyjne

Regresje PRXTAPE3 obejmują co najmniej:

1. dokładnie dwa account filters i brak all-owner filtera;
2. fail-closed unscoped Pump account;
3. config odrzucający retired `bootstrap_*` przez `deny_unknown_fields`;
4. pięć lane'ów, jedną durable boundary, epoch change i incomplete full block;
5. odrzucenie schema-2 raw przed exact outputem;
6. Create po boundary oraz późniejszy exact Buy z streamed predecessorem;
7. typed first-trade missing predecessor bez RPC repair;
8. literalne run-minimum: `9_999` blokuje, `10_000` albo 30 min przechodzi;
9. publiczne `PRXTAPE3 Complete → Qualified → complete outcome-blind window`;
10. brak outputu dla `Blocked` oraz unscoped candidate.

Pierwszy niezależny, read-only review pełnego diffu zakończył się PASS bez
findingów P0/P1/P2. Późniejszy lokalny sealed preflight, nadal bez provider
I/O, ujawnił niezgodność wyłącznie w opisowych polach `SUBSCRIBE_SENT`:
realny request oraz jego fingerprint miały dwa V1.1 account filters, ale
diagnostyka stale deklarowała dawny all-owner contract. Korekta wiąże te dwa
log fields z testowanymi stałymi V1.1. Świeży independent review tej korekty
zakończył się PASS (P0=0, P1=0, P2=0); może ona wejść wyłącznie przez
amendment niepushniętego, allowlist-only lokalnego commita.

Bundle utworzony przed tą korektą pozostaje immutable evidence, lecz nie może
być użyty do capture'u. Nadal nie wolno samodzielnie wykonywać provider I/O,
capture'u, backfillu, outcome'ów strategii ani zmian aktywnego runtime'u.
Stary sealed bundle i stary incomplete PRXTAPE2 run pozostają historycznym
evidence bez modyfikacji. Po finalnym clean commit istniejący detached operator
worktree może zostać przestawiony na nowy commit; nie jest tworzone nowe
development worktree.

## D5. Kryterium literalnego zakończenia późniejszej operacji

Research Tape V2 będzie istnieć dopiero, gdy realny, osobno autoryzowany run
spełni łącznie:

```text
PRXTAPE3 Complete
zero snapshot/GPA calls
ProgramData start/end match
jedna readiness boundary i pięć observed lanes
zero gaps/drops/reconnects
parent-linked full-block frontier i exact filtered/full-block reconciliation
denominator >= 10_000, exact coverage >= 999000 ppm
exact births/trajectories/trades-with-both-states > 0
ExactStateCapabilityV2 = Qualified
complete outcome-blind windows > 0
```

Przy `Incomplete` lub `Blocked` zachowuje się typed receipt i zatrzymuje
pipeline. Nie wolno obniżać progu, zmniejszać denominatora, dopinać snapshotu,
wykonywać RPC backfillu, imputować stanów, kwalifikować starego runu ani
uruchamiać strategy outcomes.
