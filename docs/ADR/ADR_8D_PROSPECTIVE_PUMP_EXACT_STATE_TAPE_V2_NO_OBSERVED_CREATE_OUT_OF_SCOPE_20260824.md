# ADR-8D: Prospective Pump Exact-State Tape V2 — post-boundary trade bez observed Create poza kohortą narodzin

**Data:** 2026-08-24

**Status:** IMPLEMENTED / SELF-REVIEW PASS / IMMUTABLE RAW UNCHANGED / ONE OFFLINE REQUALIFICATION PENDING

**Typ:** ADR-8D / standalone prospective V2 offline qualifier / minimalna
korekta scope prospective-birth cohort

> Globalny szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie jest dostępny w tym
> środowisku. Dokument zachowuje lokalny układ ADR-8D używany przez istniejące
> ADR-y V2.

## D0. Potwierdzony problem

Pierwsza offline qualification zachowanego, kompletnego PRXTAPE3 zachowała
pełny globalny ledger, ale zaklasyfikowała `14 280` rooted successful
post-boundary candidates jako
`unproven_post_boundary_curve_mutation_blocker`. Wszystkie były
`supported_exact_trade`; żadna nie była structuralnym `Create`/`CreateV2` ani
global dependency mutation.

Poprzednia reguła wymagała dla trade'u retained warm-up predecessor, aby
uznać curve za starą. To jest za mocne względem literalnej definicji scope:
denominator ma obejmować tylko mutation curve'ów **urodzonych po readiness
boundary**, a nie każdą curve, dla której warm-up nie utrzymał osobnego
recordu.

## D1. Decyzja

Raw universe pozostaje pełny i immutable. PRXTAPE3 zachowuje filtered Pump
transaction lane oraz unfiltered full-block lane, których reconciliation jest
warunkiem raw authority. W tym kompletnym source window brak structurally
recognized `Create`/`CreateV2` dla curve oznacza wyłącznie:

```text
curve nie należy do prospective birth cohort
→ jej post-boundary candidate = pre_existing_curve_out_of_scope
→ retained w globalnym coverage / occurrence ledgerze
→ nie jest liczony do birth-cohort denominatora
```

Nie jest to snapshot, backfill, imputacja ani rekonstrukcja stanu konta.
Jest to wyłącznie klasyfikacja przynależności do populacji zdefiniowanej przez
retained, prospective birth evidence.

Fail-closed granica pozostaje dosłowna:

- structurally recognized `Create`/`CreateV2`, nawet z malformed payloadem,
  wchodzi do kohorty i nie może zostać ukryty poza denominatorem;
- unknown Pump occurrence nadal jest globalnie zachowany i blokuje
  `MutationInventoryIncomplete`;
- `KnownReserveOrDependencyUnsupported` bez curve identity nadal jest
  `unscoped_curve_mutation_blocker`;
- każde `global_dependency_mutation` nadal blokuje capability;
- complete raw/full-block reconciliation, global occurrence conservation,
  denominator reconciliation, `999000 ppm` i qualification minimum nie
  zmieniają się.

## D2. Rewizja offline-derived authority

Zmienia się wyłącznie znaczenie exact artifactu. Aby artefakt starego scope
nie został przyjęty przez późniejszy strategy/export validator, nowe wartości
są:

```text
qualification scope          = prospective_birth_cohort_v2
capability receipt schema    = 5
exact output schema          = 4
outcome-blind window schema  = 5
```

Nie zmieniają się:

```text
raw storage                  = PRXTAPE3
raw/run/config schema        = 3
source request               = stream-only BondingCurve + canonical Global
Pump semantics / vendored IDL = bez zmian
```

## D3. Regresje

Publiczny fixture z realnym PRXTAPE3 writerem pokrywa post-boundary
`supported_exact_trade` curve bez retained warm-up recordu i bez observed
Create. Candidate pozostaje globalnie policzony,
`pre_existing_curve_out_of_scope`, nie zmienia cohort denominatora i nie
uniemożliwia Qualified fixture.

Istniejące regresje nadal sprawdzają, że:

1. malformed prospective Create pozostaje w denominatorze i blokuje;
2. unknown Pump occurrence obok old-curve trade'a nadal blokuje;
3. global dependency mutation nadal blokuje;
4. strategy/export ponownie wiąże scope coverage z immutable raw, więc
   nie może relabelować raw-derived candidate'ów;
5. `Blocked` nie publikuje outcome-blind window ani `.partial` outputu.

## D4. Zakres wyłączony

Korekta nie zmienia i nie uruchamia:

- istniejącego raw PRXTAPE3, jego segmentów, manifestu ani completion receipt;
- capture'u, preflightu, Yellowstone, RPC, provider I/O;
- GPA, snapshotu, `getMultipleAccounts`, backfillu, imputacji ani repairu;
- source requestu, configu operatora, ProgramData semantics, vendored IDL,
  Gatekeepera, OracleRuntime, execution, V1/GO-D lub strategy outcome'ów.

Po local validation i independent review dozwolona jest wyłącznie jedna nowa,
create-new offline qualification tego samego immutable raw. Jeżeli wynik
pozostanie `Blocked`, zachowany typed receipt kończy etap; nie ma recapture'u
ani obniżania bramek.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```
