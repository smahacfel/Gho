# ADR-8D: Prospective Pump Exact-State Tape V2 — zamknięta kompatybilność legacy Buy z opcjonalnymi flagami bool

**Data:** 2026-08-24

**Status:** IMPLEMENTED / LOCAL VERIFICATION PASS / SELF-REVIEW PASS /
IMMUTABLE RAW UNCHANGED / OFFLINE REQUALIFICATION PENDING

**Typ:** ADR-8D / standalone prospective V2 offline semantics / lokalna korekta
decoder–IDL compatibility

> Globalny szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie jest dostępny w tym
> środowisku. Dokument zachowuje lokalny układ ADR-8D używany przez istniejące
> ADR-y V2.

## D0. Potwierdzony problem

Kompletna, immutable taśma PRXTAPE3 zawiera udane wykonania pod przypiętym
Pump ProgramData dla obu legacy dyskryminatorów:

```text
buy              = global:buy
buy_exact_sol_in = global:buy_exact_sol_in
```

Raw zachowuje trzy literalne długości argumentów po ośmiobajtowym
dyskryminatorze:

```text
16 B = dwa canonical u64, bez feature byte
17 B = dwa canonical u64 + jeden bool
18 B = dwa canonical u64 + dwa bool
```

Każda forma występuje w `success=true`, również jako inner CPI z zachowanym
Anchor `TradeEvent`. Aktualny vendored publiczny IDL opisuje wyłącznie formę
17-bajtową jako `track_volume: OptionBool`. Poprzedni strict decoder traktował
formę 16-bajtową jako `V2 Borsh bool is truncated`, a formę 18-bajtową jako
trailing bytes. To fałszywie zmieniało kompletne, udane mutacje w malformed
inventory oraz blokowało offline qualification.

Nie jest to dowód uszkodzenia raw: raw ma kompletne receipts, ProgramData
start/end match oraz successful source transaction. Jest to wąska różnica
między ekspresją aktualnego IDL a zachowaną, execution-observed kompatybilną
gramatyką wdrożonego programu.

## D1. Decyzja

Zachowujemy standardowy pełny strict Borsh decode dla wszystkich instrukcji i
eventów. Dodajemy wyłącznie zamknięty wyjątek dla literalnych nazw:

```text
buy
buy_exact_sol_in
```

Wyjątek działa tylko wtedy, gdy załadowany z vendored IDL kontrakt nadal ma
dokładnie:

```text
dwa określone u64 argumenty
+ track_volume: OptionBool
```

Dozwolona jest wyłącznie dokładna forma:

```text
u64 + u64 + 0..=2 feature bytes
```

Każdy feature byte musi być `0` albo `1`. Trzeci trailing byte, zła wartość,
inna instrukcja albo future IDL drift pozostają fail-closed.

## D2. Granica authority

Ten wyjątek nie nadaje feature bytes żadnej authority dla state, parenta,
mint, usera, quote regime ani Event-CPI:

- forma 17 B może udostępnić jednoznaczne `track_volume` jako IDL argument;
- forma 16 B nie tworzy nieobserwowanej wartości `track_volume`;
- forma 18 B tylko zużywa oba bool bytes; nie przypisuje żadnego z nich do
  `track_volume`, ponieważ takie przypisanie byłoby imputacją;
- jeżeli przyszły manifest będzie wymagał `ParentInstructionArgument` dla
  `track_volume`, forma 16 B i 18 B odpadnie na braku tej authority.

Canonical exact state nadal pochodzi wyłącznie z finalnego streamed
BondingCurve anchoru. Event-CPI nadal musi przejść strict event decode,
bezpośredni Pump parent, canonical event authority i manifest-pinned binding.

## D3. Regresje

Testy obejmują:

1. 16 B, 17 B i 18 B dla realnego vendored `buy`;
2. 16 B dla `buy_exact_sol_in`;
3. invalid bool, trzeci extension byte i trailing byte dla nieobjętego `buy_v2`
   jako fail-closed;
4. publiczną ścieżkę PRXTAPE3 writer → qualify dla post-boundary legacy
   `buy` 16 B z prawdziwym inner `TradeEvent`, exact final anchor i atomicznym
   exact outputem.

## D4. Zakres wyłączony

Korekta nie zmienia:

- immutable raw PRXTAPE3, segmentów, start manifestu ani completion receipt;
- source requestu, recorder, full-block reconciliation ani birth-cohort scope;
- vendored IDL, manifestu semantics, ProgramData hash ani account layoutów;
- capture'u, preflightu, Yellowstone, RPC, provider I/O;
- GPA, snapshotu, backfillu, imputacji, repairu;
- Gatekeepera, OracleRuntime, execution, V1/GO-D ani strategy outcomes.

Po local verification i independent review dozwolona jest wyłącznie jedna nowa
create-new **offline** qualification tego samego raw. `Blocked` pozostaje
kontraktowym wynikiem i nie daje prawa do exportu ani nowego capture'u.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```
