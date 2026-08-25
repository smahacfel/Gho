# ADR-8D: Prospective Pump Exact-State Tape V2 — inner Event-CPI privileges and transactional final state

**Date:** 2026-08-24

**Status:** IMPLEMENTED LOCALLY / TARGETED OFFLINE VALIDATION PASS / IMMUTABLE RAW UNCHANGED

**Type:** ADR-8D / standalone prospective V2 offline qualifier / fail-closed evidence interpretation

> Globalny szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie jest dostępny w tym
> środowisku. Dokument zachowuje lokalny układ ADR-8D używany przez istniejące
> ADR-y V2.

## D0. Potwierdzony problem

Read-only analiza kompletnego PRXTAPE3 ujawniła dwie niezależne fałszywe
ścieżki `Unknown` dla Anchor `emit_cpi!` w offline qualifierze. Nie są to
błędy recordera, raw, Yellowstone requestu ani source completeness.

1. `SubscribeUpdateTransaction` zachowuje transaction-message account flags,
   ale nie zachowuje invocation-local `AccountMeta` privileges wewnętrznego
   CPI. Canonical `__event_authority` może więc być writable w message-wide
   privilege union, mimo że konkretny one-account Anchor Event-CPI envelope
   nie dostarcza żadnej local writable authority do sprawdzenia.

2. Curve account update związany z signature opisuje stan końcowy całej
   transakcji. Gdy wcześniejsza mutation tej samej curve emituje event, a
   późniejsza mutation tej curve występuje w tej samej signature, finalny
   anchor nie może dowodzić post-state wcześniejszej mutation. Wcześniejszy
   validator porównywał je mimo tej utraty porządku state authority i tworzył
   fałszywy `anchor_event_transport_final_state_mismatch`.

Zachowane PRXTAPE3, segmenty, start manifest, completion receipt, poprzedni
diagnostic exact artifact oraz nowy blocked diagnostic artifact pozostają
immutable. Ta decyzja nie uruchamia provider I/O, capture'u, preflightu ani
requalification.

## D1. Decyzja

### Inner Event-CPI privilege authority

Po odrzuceniu direct transportu, wewnętrzny Anchor Event-CPI nadal wymaga:

- dokładnie jednego account indexu;
- canonical PDA `__event_authority` dla pinned Pump Program;
- prawidłowego wrapper discriminatora, strict event Borsh i immediate Pump
  stack parent.

Nie interpretuje natomiast message-level `signer` ani `writable` jako
invocation-local authority. Taka interpretacja wymagałaby danych, których
retained protobuf nie przechowuje. Nie jest to rozluźnienie outer Pump
instruction contractu ani named-account contractu.

### Final state po całej transakcji

Event nadal przechodzi strict decode, immediate-parent binding i wszystkie
non-final-state semantic bindings. Final-state bindings są walidowane tylko
wtedy, gdy nie istnieje późniejsza candidate mutation tej samej curve w tej
samej signature:

- późniejszy outer instruction lub późniejsza retained inner path oznacza, że
  finalny anchor może należeć do późniejszej mutation;
- wtedy Event-CPI pozostaje validated transport, lecz nie otrzymuje
  unprovable final-state binding;
- candidate evaluation pozostaje literalnie non-exact, ponieważ transakcja z
  więcej niż jednym reserve/dependency candidate nadal dostaje
  `transaction_has_multiple_reserve_or_dependency_candidates`.

Nie powstaje nowa path do exact state, denominator nie jest zmniejszany, a
źle związany event nadal staje się `Unknown`, gdy nie ma późniejszej
same-curve candidate, która wyjaśnia brak porównywalnego final state.

## D2. Regresje

Dodano testy oparte na realnym writerze i publicznym qualifierze:

- publiczny PRXTAPE3 fixture oznacza canonical event-authority PDA writable
  w message headerze, zachowuje poprawny inner Event-CPI i nadal uzyskuje
  `Qualified` bez unknown occurrence;
- unit regression sprawdza relację wykonania: późniejsza same-curve candidate
  blokuje tylko retroaktywne przypisanie signature-final anchoru do
  wcześniejszego parenta; latest candidate i inna curve nie tracą własnej
  final-state validation;
- istniejąca regresja `transaction_candidate_count != 1` nadal wymaga
  explicit non-exact, więc validated transport nie może promować transakcji
  wielomutacyjnej do exact trajectory.

## D3. Zakres wyłączony

Korekta nie zmienia:

- PRXTAPE3 storage/schema/magic, source requestu Yellowstone, five-lane
  readiness, account filters, ProgramData receipts, full-block reconciliation
  ani raw retention;
- vendored Pump IDL, semantics manifestu, instruction Borsh consumption,
  allocation padding, account-prefix/remaining-account contractu, GPA,
  snapshotu, backfillu lub imputacji;
- coverage/denominator/minimum gates, output/window schemas, V1/GO-D,
  Gatekeepera, OracleRuntime ani execution;
- immutable raw lub jakiegokolwiek istniejącego outputu diagnosticznego.

W szczególności ta korekta nie jest uzasadnieniem do ponownej kwalifikacji
tego raw bez osobnej, późniejszej decyzji. Nie zmienia literalnego wyniku
`missing_exact_pre_anchor` dla pierwszej zaobserwowanej trade starej curve.

## D4. Weryfikacja lokalna

```text
cargo fmt --all -- --check
cargo test --locked --offline -p seer \
  public_v2_inner_event_cpi_does_not_infer_message_writable_privilege \
  --lib --no-fail-fast
cargo test --locked --offline -p seer research_exact_tape_v2_materializer \
  --lib --no-fail-fast
git diff --check
```

Następnie wymagane są kompletna właściwa macierz V2, neutralny self-review i
allowlist-only commit. Żaden etap nie wykonuje provider I/O ani nie zmienia
raw evidence.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```
