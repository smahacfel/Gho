# ADR-8D: Prospective Pump Exact-State Tape V2 — denominator kohorty narodzin po readiness boundary

**Data:** 2026-08-24

**Status:** IMPLEMENTED / SELF-REVIEW PASS / IMMUTABLE RAW UNCHANGED / ONE OFFLINE REQUALIFICATION PENDING

**Typ:** ADR-8D / standalone prospective V2 offline qualifier / fail-closed prospective-birth scope

> Globalny szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie jest dostępny w tym
> środowisku. Dokument zachowuje lokalny układ ADR-8D używany przez istniejące
> ADR-y V2.

## D0. Potwierdzony problem definicji denominatora

Stream-only PRXTAPE3 nie ma baseline snapshotu kont i nie może go uzyskać
przez GPA, RPC backfill, imputację ani inny historyczny repair. Poprzednia
definicja exact coverage używała wszystkich rooted successful curve mutations
po readiness boundary jako denominatora. Prawidłowo zachowywała globalne raw
evidence, lecz mieszała dwie różne populacje:

- curve'y, których Create zostało po raz pierwszy zaobserwowane prospektywnie
  po boundary;
- curve'y żyjące już przed boundary, dla których stream nie posiada
  dozwolonego predecessor state.

Brak predecessor state starej curve jest typed non-exact i musi pozostać
widoczny w raw/coverage. Nie może jednak obniżać coverage kohorty, dla której
sam stream posiada prospektywny birth evidence. Jednocześnie nie wolno
osiągnąć wyższego coverage przez dyskretne usunięcie starych lub nieznanych
mutacji z globalnego occurrence ledgeru.

## D1. Decyzja

Raw source universe pozostaje pełny. Recorder, PRXTAPE3, request Yellowstone,
five-lane readiness, parent-linked full-block frontier i reconciliation nie
zmieniają się.

Offline qualifier buduje wyłącznie z immutable raw `prospective_birth_cohort`:

1. curve należy do kohorty, jeżeli jej pierwszy rooted successful candidate
   jest structurally recognized `Create`/`CreateV2` po
   `cohort_slots_strictly_after`;
2. nie istnieje retained rooted mutation ani canonical BondingCurve anchor tej
   curve na lub przed boundary;
3. nie istnieje drugi retained Create tej samej curve, jeżeli curve poza tym
   kwalifikowałaby się jako prospective birth; powtórzone rekordy curve już
   widzianej przed boundary pozostają typed out-of-scope, a nie nowym
   blockerem kohorty;
4. canonical ordering obejmuje slot, transaction index, signature oraz locator
   outer/inner instruction, więc późniejszy Create w tej samej transakcji nie
   naprawia wcześniejszej mutation.

Kohorta jest identyfikowana przez structural Create, nie tylko przez already
exact Create. Nieprawidłowy Event-CPI, account vector lub final anchor może
uczynić birth non-exact i zablokować qualification, ale nie może ukryć tej
curve poza denominatorem.

`successful_rooted_mutation_denominator` oznacza odtąd dokładnie wszystkie
rooted successful mutations curve'ów tej kohorty. Coverage nadal wymaga
literalnego `999000 ppm` i przechodzi tylko gdy:

```text
exact rooted cohort mutations
-------------------------------- >= 999000 ppm
all rooted successful cohort mutations
```

Pełny globalny ledger nadal obejmuje wszystkie rooted successful source
occurrences, w tym warm-up, pre-boundary i post-boundary mutations starych
curve'ów. Coverage JSONL zapisuje dla każdego candidate jeden typed scope:

- `prospective_birth_cohort` — liczony do denominatora;
- `pre_boundary_out_of_scope` — retained warm-up/pre-boundary mutation;
- `pre_existing_curve_out_of_scope` — post-boundary mutation curve już
  potwierdzonej canonical anchor albo rooted candidate przed boundary;
- `global_dependency_blocker` — globalny dependency mutation, który nadal
  blokuje capability;
- `unproven_post_boundary_curve_mutation_blocker` — post-boundary mutation z
  curve identity, ale bez retained pre-boundary evidence i bez jednoznacznego
  observed Create; nie może zostać relabelled jako pre-existing, ponieważ
  mogłaby należeć do pominiętego lub malformed prospective birth;
- `unscoped_curve_mutation_blocker` — każdy candidate bez bezpiecznej curve
  identity; `KnownReserveOrDependencyUnsupported` nie jest dowodem, że
  mutation jest non-curve, więc również pozostaje blockerem;
- `outside_rooted_successful_universe` — failed albo non-rooted diagnostic
  occurrence.

Partition jest ponownie liczony z descriptor-pinned coverage JSONL przed
przyjęciem Qualified artifactu. Strategy-input validator ponownie buduje
kohortę z anonymous, receipt-bound raw descriptors i streamingowo porównuje
scope każdego coverage candidate z jego immutable raw transaction; nie ufa
samemu labelowi Qualified, self-consistent receiptowi ani mutable raw pathname.
Receipt, exact manifest i window manifest wiążą
`qualification_scope = prospective_birth_cohort_v1`.

## D2. Zachowane fail-closed bramki

Następujące warunki nadal blokują capability niezależnie od cohort coverage:

- global occurrence conservation;
- full-block/filter reconciliation oraz parent-linked finalized frontier;
- unknown/malformed occurrence w cohort, unscoped curve mutation lub
  curve o nieudowodnionym post-boundary birth albo otherwise unclassifiable
  rooted successful source evidence;
- każde `global_dependency_mutation`;
- ambiguous/repeated prospective Create;
- exact coverage poniżej `999000 ppm`, brak exact birth, trajectory albo
  trade z obiema state;
- minimum kwalifikacji: 30 min **lub** 10 000 scoped rooted cohort mutations.

Pre-boundary i pre-existing curve mutations nie znikają: są przechowywane,
liczone globalnie i zapisane w typed coverage. Nie uzyskują tylko prawa do
zmiany denominatora kohorty, dla której nie mogłyby mieć legalnego
stream-observed predecessor state.

## D3. Rewizja artefaktów kwalifikacji

Ponieważ znaczenie denominatora jest authority dla późniejszego exportu,
zmieniają się wyłącznie offline-derived schema versions:

```text
capability receipt schema  = 4
exact output manifest      = 3
outcome-blind window       = 4
```

PRXTAPE3, raw storage format, source capture semantics, semantic manifest i
vendored IDL nie zmieniają się. Poprzedni diagnostic exact artifact zachowuje
historyczną wartość i nie może udawać artifactu w nowym scope.

## D4. Regresje

Publiczne fixturey oparte na rzeczywistym writerze i qualifierze pokrywają:

1. warm-up trade innej, istniejącej curve pozostaje globalnym i
   `pre_boundary_out_of_scope`, podczas gdy post-boundary Create + Buy nowej
   curve daje denominator `2`, exact coverage `1_000_000 ppm` i Complete
   outcome-blind window;
2. post-boundary trade curve widzianej przed boundary pozostaje
   `pre_existing_curve_out_of_scope`, jest zachowany w globalnym ledgerze i
   nie zmienia denominatora kohorty;
3. retained rooted `initialize` jest `global_dependency_blocker`, publikuje
   typed Blocked receipt i nie może zostać ukryty przez cohort scope;
4. uszkodzony Event-CPI prospective Create pozostaje w denominatorze,
   ale jest non-exact i blokuje qualification zamiast wypaść z kohorty;
5. strategy/export rewaliduje scope partition oraz raw-derived cohort przez
   anonymous descriptor snapshot przed publikacją; self-consistent relabel
   `pre_boundary_out_of_scope` → `pre_existing_curve_out_of_scope` jest
   odrzucany przed outputem.
6. dwa post-boundary `Create` curve już potwierdzonej przez warm-up anchor
   pozostają `pre_existing_curve_out_of_scope`; nie tworzą fałszywego konfliktu
   prospective birth ani nie znikają z globalnego candidate census.
7. unknown Pump occurrence obok znanego trade starej curve pozostaje globalnym
   `MutationInventoryIncomplete`; typed scope znanego candidate nie może
   przykryć niezidentyfikowanej mutation.
8. post-boundary Buy curve bez retained pre-boundary evidence i bez observed
   Create publikuje typed Blocked receipt; nie może zostać zaklasyfikowany jako
   `pre_existing_curve_out_of_scope` ani zmniejszyć denominatora.

## D5. Zakres wyłączony

Korekta nie zmienia i nie uruchamia:

- istniejącego raw PRXTAPE3, jego segmentów, manifestu lub completion receipt;
- nowego capture'u, sealed capture preflightu, Yellowstone, RPC, provider I/O;
- snapshotu kont, GPA, getMultipleAccounts, backfillu, imputacji ani repairu;
- source requestu, configu operatora, ProgramData semantics, vendored IDL,
  semantics manifestu, Gatekeepera, OracleRuntime, execution, V1/GO-D lub
  strategii/outcome'ów.

Po local validation, self-review i allowlist-only commicie dozwolona jest
jedna create-new offline requalification zachowanego raw. Nie jest dozwolony
recapture. Jeśli wynik będzie `Blocked`, zachowany typed receipt kończy etap;
nie obniża się progów i nie robi repairu danych.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```
