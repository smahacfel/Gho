# ADR-8D: Pump Research — kuracja reużywalnego evidence basis z OBS Lite i ACE

**Data:** 2026-08-17

**Status:** IMPLEMENTED / LOCAL-ONLY / NO PROVIDER I/O / NO STRATEGY POLICY

**Task:** `PUMP_RESEARCH_REUSABLE_OBS_ACE_EVIDENCE_FOUNDATION`

## D0. Problem

Dirty worktree zawierał równolegle implementację Pump Research Evidence Tape
V1.1 oraz starsze zmiany powstałe podczas walidacji OBS Lite, A0 i ACE. Pełne
wciągnięcie tych zmian do jednego PR przeniosłoby progi, lease'y, terminalizery
i aktywne capture lanes konkretnych strategii. Całkowite ich odrzucenie
usunęłoby natomiast source-backed poprawki parsera potrzebne w kolejnych
badaniach, w szczególności poprawny `CreateV2`, Mayhem i pełne rezerwy.

Potrzebna była kuracja według ownershipu danych, nie według historycznej nazwy
strategii.

## D1. Decyzja

Do PR wchodzi wyłącznie reużywalna, outcome-blind warstwa źródłowa:

- rozdzielenie legacy `Create` i bieżącego `CreateV2`;
- wariantowy układ kont, w tym właściwa pozycja `user`;
- ścisłe dekodowanie obecnego oraz znanego pre-cashback layoutu `CreateV2`;
- bezpośrednie, tri-state facts Mayhem i Cashback bez imputacji;
- richer `CreateEvent` z virtual/real reserve fields;
- stabilny prefix `TradeEvent` z real reserve fields i fail-closed truncated
  tail;
- `PumpCreationRegimeV1` jako wersjonowany zestaw faktów źródłowych;
- exact initial virtual quote reserves;
- source-backed canonical birth order;
- mutation inventory używany przez Pump Research materializer;
- minimalne, neutralne uzupełnienia konstruktorów wymagane przez addytywne
  pola.

Nie wchodzi żaden predicate wybierający kohortę strategii. Usunięto
`is_regular_native_sol_non_mayhem`: typ przechowuje fakty, a prerejestrowany
eksperyment definiuje własną selekcję.

## D2. Create i CreateV2

Parser zachowuje osobne discriminators:

```text
global:create    -> DISC_CREATE
global:create_v2 -> DISC_CREATE_V2
```

`CreateV2` nie może fallbackować do legacy layoutu po błędzie decode. Znany
pre-cashback layout jest obsługiwany jawnie i zwraca
`create_cashback = None`, a nie wymyślone `false`.

Direct instruction i matching `CreateEvent` są porównywane. Konflikt Mayhem
albo niekompletne birth evidence daje `Unknown`; nie rozstrzyga go downstream
trade, wall clock ani ręczny default.

## D3. Rezerwy i kolejność

Źródłowe liczby całkowite pozostają odrębne od display-oriented liquidity:

- initial virtual/real reserves z create evidence;
- post-trade virtual/real reserves z current `TradeEvent` prefix;
- exact initial virtual quote reserves;
- canonical order z pełnego tuple:
  `slot + tx_index + outer_instruction_index + inner_instruction_path +
  semantic_event_ordinal`.

Częściowego locatora nie promuje się do canonical order. Nieznany future event
tail pozostaje opaque; truncated obecny reserve prefix jest błędem.

## D4. Granica wyłączenia

Jawnie poza PR pozostają:

- OBS Lite anchor thresholds, 48/50 SOL funnel, 482.5 s tail i evidence lease;
- OBS/A0 census, terminalizery, outcome evaluators, monitor i rollout configs;
- ACE/A0 `FullUniverseTradeEvidence`, capture disposition receipts i osobna
  launcherowa reserve-state lane;
- candidate-scoped capture leases i runtime retention;
- local-gap journal/control-plane przebudowa niezależna od tape;
- rozszerzenia `DetectedPool`, EventWriter i OracleRuntime służące staremu
  sposobowi zbierania strategio-specyficznego evidence;
- progi, SELECTED/REST, entry/exit, PnL, TP/SL/TIMEOUT oraz jakakolwiek zmiana
  Gatekeepera, Triggera lub execution;
- RPC backfill oraz GO-E jako gate.

GO-D frozen tape przejął authority dla przyszłych przypisanych walidacji.
Dlatego historyczny active-runtime capture wiring nie jest reużywalną
zależnością tego PR i nie może być przywrócony tylko po to, by odtworzyć stary
eksperyment.

## D5. Wpływ na runtime

Zmiana rozszerza źródłowy model zdarzeń addytywnie i zachowuje neutralne
`Unknown`/`None` w alternatywnych connectorach oraz fixture'ach. Nie zmienia:

- subskrypcji Yellowstone;
- runtime admission;
- `MaterializedFeatureSet`;
- Gatekeepera;
- polityki strategii;
- shadow/live boundary;
- execution.

## D6. Weryfikacja granicy

Kuracja jest sprawdzana przez:

- strict Create/CreateV2 i malformed-no-fallback tests;
- current oraz pre-cashback CreateV2 fixtures;
- Mayhem/Cashback tri-state assertions;
- current CreateEvent/TradeEvent reserve-prefix tests;
- canonical birth-order tests;
- mutation-inventory tests;
- frozen parser projection/parity;
- CS0 i frozen Pump Research V1 corpus;
- clean candidate build/test checks;
- staged-diff scan wykluczający OBS/ACE/A0 runtime policy markers.

Końcowe wyniki lokalne:

```text
CreateV2 / Mayhem / no-fallback corpus           6 passed
current TradeEvent real-reserve prefix           2 passed
transaction-local mutation inventory             2 passed
PumpCreationRegimeV1 source facts                 1 passed
canonical birth order complete/partial tuples     1 passed
parser parity                                     1 passed
frozen Pump Research V1                          11 passed
CS0                                               2 passed
seer lib/bin checks                               PASS
ghost-launcher lib check                          PASS
cargo fmt                                         PASS
```

Workspace-wide `--all-targets` ma w bazie wcześniejsze, niezależne błędy w
starych benchmarkach i fixture'ach. Nie są naprawiane w tym PR; istotne
pakiety i kontrakty są weryfikowane osobno.

## D7. Authority operacyjna

```text
GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```

Ten ADR nie autoryzuje nowego capture'u, provider I/O, RPC backfillu,
otwarcia outcome'ów ani zmiany runtime'u.
