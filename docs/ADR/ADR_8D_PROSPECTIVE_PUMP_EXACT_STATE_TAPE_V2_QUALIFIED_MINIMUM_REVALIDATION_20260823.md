# ADR-8D: Prospective Pump Exact-State Tape V2 — rewalidacja minimum kwalifikacji przy strategy-input

**Data:** 2026-08-23

**Status:** IMPLEMENTED LOCALLY / RELEVANT OFFLINE MATRIX PASS / NEUTRAL SELF-REVIEW PASS / FRESH INDEPENDENT REVIEW PASS (P0=0, P1=0, P2=0) / NO PROVIDER I/O / NO RAW V3 CREATED

**Typ:** ADR-8D / offline exact-artifact authority / fail-closed qualification gate

> Globalny szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie był dostępny ani w
> tym checkoutie, ani pod wskazaną ścieżką. Dokument zachowuje lokalny układ
> ADR-8D używany przez wcześniejsze ADR-y V2.

## D0. Problem i decyzja

PRXTAPE3 qualifier poprawnie wylicza literalne minimum prospective cohort:

```text
cohort elapsed >= 1_800_000 ms
OR
successful rooted mutation denominator >= 10_000
```

Niespełnienie tego warunku tworzy typed blocker
`QualificationRunBelowMinimum` i uniemożliwia normalne utworzenie
`Qualified`. Wcześniejszy walidator exact artifactu sprawdzał jednak status,
blockery, coverage, denominator, births i trajectories, lecz nie rewalidował
samego pola `qualification_run_below_minimum` przed strategy-input i
outcome-blind exportem.

Decyzja: `Qualified` pozostaje tylko etykietą pomocniczą. Strategy-input
authority musi ponownie dowieść zarówno, że receipt nie deklaruje runu poniżej
minimum, jak i że ta deklaracja odpowiada aktualnemu immutable raw completion
receipt oraz denominatorowi exact artifactu.

## D1. Fail-closed kontrakt

`validate_exact_output_receipt_v2()` odrzuca teraz każdy artifact, dla którego
zachodzi choćby jeden z warunków:

```text
status != Qualified
blockers != []
qualification_run_below_minimum == true
```

Następnie `validate_qualified_exact_output_binding_v2()` oblicza z raw
authority niezależnie:

```rust
expected_below_minimum = qualification_run_below_minimum_v2(
    raw.completion_receipt.cohort_capture_elapsed_ms,
    exact.receipt.successful_rooted_mutation_denominator,
)
```

i wymaga równocześnie:

```text
exact.receipt.qualification_run_below_minimum == expected_below_minimum
expected_below_minimum == false
```

W konsekwencji nie przejdą ani artefakt jawnie oznaczony jako poniżej minimum,
ani artefakt z etykietą `Qualified`, który po spójnym przepisaniu digestów
próbuje ukryć zbyt krótki raw cohort lub zbyt mały denominator.

## D2. Regresje

Dwa publiczne regresyjne przypadki przechodzą przez realny PRXTAPE3 writer,
offline qualification i publiczny `validate_prospective_exact_state_strategy_input_v2()`:

1. `Qualified` receipt z `qualification_run_below_minimum = true`, przy
   poprawnie przeliczonym manifeście exact artifactu, jest odrzucany zanim
   stanie się strategy-input authority.
2. Raw completion receipt z elapsed `1_799_999 ms` i denominatorem `2`, przy
   zachowanym `Qualified` receipt flag `false` oraz poprawnie przeliczonym
   source-control/manifest digest chain, jest odrzucany, ponieważ adapter
   sam wyprowadza `expected_below_minimum = true` z raw authority.

Testy nie wykonują RPC, Yellowstone, preflightu, capture'u ani eksportu
outcome'ów strategii. Zmieniają wyłącznie tymczasowe local fixtures i przy
każdej modyfikacji ponownie wiążą odpowiednie digesty, aby test nie odpadał
wcześniej na banalnym naruszeniu integralności plików.

## D3. Zakres wyłączony

Korekta nie zmienia:

- recordera, PRXTAPE3 magic/storage/schema ani stream-readiness boundary;
- Yellowstone requestu, dwóch filtrów kont, full-block lane ani source
  reconciliation;
- configu, Pump IDL, semantics manifestu, ProgramData RPC czy operator
  preflightu;
- exact JSONL/window schema, denominatora, coverage threshold ani typed
  blocker taxonomy;
- V1/GO-D, GO-E, aktywnego Seera, Gatekeepera, OracleRuntime lub execution.

Nie powstaje nowy worktree, sealed bundle, raw tape, exact output rzeczywisty
ani strategy outcome. Istniejący bundle pozostaje immutable historical
evidence i nie jest modyfikowany.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```

## D4. Kolejna bramka

Relevant offline matrix po korekcie przeszła:

```text
cargo fmt --all -- --check                                      PASS
cargo check --locked --offline -p seer --bin pump-exact-state-tape-v2
                                                                  PASS
research_exact_tape_v2_materializer                              23/23 PASS
research_exact_tape_v2                                           72/72 PASS
locked/offline release build                                     PASS
release --help                                                   PASS
git diff --check                                                 PASS
git diff --cached --check                                        PASS
untracked ADR whitespace check                                   PASS
```

Świeży, read-only independent review potwierdził PASS bez findingów P0/P1/P2:
sprawdził literalne rejections, raw-to-receipt rewalidację minimum, oba
call-site'y konsumenckie, digest-preserving regresje oraz brak driftu source
contract/runtime. Jedynym następnym krokiem kodowym jest jeden mały,
allowlist-only commit potomny od `9c70ba2`. Dopiero wtedy istniejący detached
operator worktree może zostać przestawiony na ten commit. Replacement sealed
preflight oraz bounded capture pozostają osobnymi decyzjami operatorskimi.
