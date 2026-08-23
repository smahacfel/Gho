# ADR-8D: Prospective Pump Exact-State Tape V2 — authority kwalifikacji offline

**Data:** 2026-08-22

**Status:** REVISED LOCALLY / P0 I RETAINED-PROTOBUF P1 COVERED / FINALIZED SLOT.PARENT SUCCESSOR INDEPENDENT REVIEW PASS / ALLOWLIST-ONLY COMMIT AUTHORIZED / V2 RAW NOT CREATED / NO PROVIDER I/O

**Typ:** ADR-8D / prospective research evidence / offline qualification authority

## D0. Decyzja

Tape V2 ma dwa rozłączne progi authority:

```text
raw V2 Complete
  !=
ExactStateCapabilityV2 = Qualified
```

Raw `Complete` oznacza wyłącznie poprawnie nagrany, prospektywny source
contract. Dopiero oddzielny, offline qualifier może utworzyć atomiczny
artifact `Qualified`. Wynik `Blocked` jest trwałym diagnostycznym evidence,
ale nigdy nie otwiera strategii, outcome'ów, exportu ani aktywnego runtime'u.

Nie zmienia to authority historycznego GO-D:

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```

V2 nie naprawia, nie modyfikuje ani nie reinterpretuje V1/GO-D.

## D1. Semantyka musi istnieć przed capture

Operator config V2 wymaga operator-controlled, regularnej non-symlink ścieżki
do jednego hash-pinned semantics manifestu. Manifest wiąże vendored public Pump IDL, jego digest, semantics ID,
pełne layouty accountów, oczekiwany BLAKE3 finalized ProgramData oraz
literalny Event-CPI parent contract dla **każdego** eventu z IDL.

W schema semantics v7 każdy event wskazuje dozwolone parent instructions i
wiąże każde własne pole dokładnie raz z jedną z authority:

```text
parent AccountMeta role
parent instruction argument (identyczne Borsh bytes)
hash-pinned literal Borsh
final anchored BondingCurve state
StrictDecodeOnly
```

`StrictDecodeOnly` nie jest piątą authority exact-state. Oznacza wyłącznie,
że dane pole istnieje w przypiętym IDL i zostało w całości, bez trailing bytes,
zdekodowane. Jest przeznaczone dla dynamicznej telemetrii eventu — np.
timestampów, wolumenów lub vectorów shareholders — której nie da się uczciwie
wyprowadzić z parent instruction albo finalnego curve state. W v7 obejmuje też
`virtual_sol_reserves`, `virtual_quote_reserves` i `real_quote_reserves`, gdy
ich semantyka zależy od quote regime danego wariantu. Nie mogą one być użyte do
identity parenta ani do state proof, dopóki kolejna pinned semantics revision
nie dostarczy wariantowo udowodnionego contractu. Canonical exact state nadal
pochodzi wyłącznie z finalnego BondingCurve anchoru.

Ten sam manifest wskazuje dla każdej `supported_exact_trade` oraz
`supported_exact_create` literalne role `bonding_curve` i `mint` w pinned
AccountMeta vectorze. Qualifier nie ma fallbacku do nazw, pozycji ani aliasów
zaszytych w Rust; brak lub drift tej mapy zatrzymuje preflight/kwalifikację.

Sam poprawny Anchor wrapper, PDA, stack height i pełne Borsh payloadu nie
tworzą event authority. Nieznany parent variant, brak pola, rozbieżność
identity/amount/state lub brak final anchor staje się typed `Unknown`, a więc
`MutationInventoryIncomplete` — nigdy `ValidatedEventTransport`.

Local-only preflight zapisuje digests manifestu i IDL oraz oczekiwany ProgramData
w sealed receipt. Capture ponownie ładuje dokładnie ten sam authority object i
przed utworzeniem run directory i Yellowstone streamem odczytuje wymagany
finalized ProgramData przez jawnie skonfigurowany source-provider bootstrap
RPC, po czym wymaga:

```text
observed_finalized_program_data_blake3
  ==
preflight_pinned_expected_program_data_blake3
```

Start manifest raw V2 zachowuje semantics ID, manifest digest, IDL digest i
expected ProgramData. Qualifier może ponownie przeczytać semantics manifest
wyłącznie po to, by potwierdzić literalną zgodność z raw start manifestem; nie
ma ścieżki wyboru IDL, rewizji ani ProgramData po obejrzeniu raw danych.

Checkout zawiera lokalny, testowany manifest i vendored public IDL dla
konkretnego commita Pump. Nie są one jednak sealed operator authority dla
przyszłego capture'u: nie istnieje jeszcze private operator config, sealed
preflight ani observed finalized ProgramData receipt. Nie wolno uruchamiać
preflight/capture na podstawie fixture'ów, ani zastępować wymaganej operator
authority syntetycznym manifestem.

## D2. Offline raw authority i snapshot

`qualify` jest standalone, offline-only command. Czyta tylko:

1. complete V2 raw run;
2. jawnie podany semantics manifest;
3. kernel-bound `/proc/self/exe` bieżącego materializera.

Przed semantyczną interpretacją sprawdza V2 start/completion controls,
contiguous segment receipts, framing, footer, whole-file SHA-256/BLAKE3,
prefix chain, sealed bootstrap/readiness overlap, wymagane lane'y oraz
full-block ↔ filtered-Pump transaction multiset.

Następnie kopiuje każdy receipt-bound segment do Linux `O_TMPFILE`, weryfikuje
pełne hashe i czyta dalsze rekordy wyłącznie z zachowanych read-only
descriptorów. Źródłowe pathname'y nie są później używane do materializacji.
Ogranicza to raw-A → raw-B TOCTOU bez tworzenia nazwanego staging directory.

Przed pierwszym anonymous snapshotem qualifier wymaga wolnego miejsca co
najmniej:

```text
sum(receipt.file_bytes)
  + raw_start_manifest.min_free_bytes
  + 64 MiB metadata allowance
```

Brak budżetu kończy operację przed snapshotem i przed `.partial` outputem.
Nie jest to obietnica niezmienności współdzielonego filesystemu; każdy read,
copy, sync i publication nadal failuje closed przy I/O/storage error.

## D3. Exact-state capability i artefakty

Kwalifikator:

- buduje source-lossless context transaction i all-Pump-owned account anchors;
- stosuje tylko strict current semantics z pre-pinned manifestu;
- zachowuje unknown instruction/account jako non-exact evidence, bez zgadywania
  pozycji kont lub RPC/backfillu;
- uznaje Anchor `emit_cpi!` wyłącznie jako w pełni zdekodowany transport z
  właściwym PDA/AccountMeta/stack contractem, istniejącym bezpośrednim Pump
  parent occurrence oraz kompletnym hash-pinned parent/event field binding;
  event nie jest samodzielną authority dla state anchor. Wyłącznie pola
  literalnie przypisane w manifeście jako invariant `final_curve_state_field`
  są porównywane z finalnym anchorem tego parent transaction; zależne od quote
  regime reserve fields są tylko StrictDecodeOnly;
- prowadzi occurrence ledger i osobne conservation denominator;
- wymaga canonical rooted slot evidence, exact payload/account contracts i
  niezbędnych before/after anchors; same-slot predecessor jest dopuszczalny
  wyłącznie po signature → transaction-index binding i tylko gdy jego
  transaction index jest ściśle mniejszy od parenta; każdy non-bootstrap
  anchor musi dodatkowo należeć do unique finalized
  Slot/BlockMeta/full-block authority — update z niecanonical slotu pozostaje
  w raw, ale nie może stanowić `state_before` ani `state_after`;
- wymaga literalnego coverage `>= 999000 ppm`, zerowych completeness blockers
  oraz niezerowych exact trajectories, births i successful rooted exact
  **trade z state_before + state_after** dla `Qualified`.

Publikacja jest create-new i atomiczna:

```text
births_v2.jsonl
trajectories_v2.jsonl
coverage_v2.jsonl
exact_state_capability_v2.json
manifest_v2.json
```

Root ma `0700`, pliki `0600`, każdy artifact ma SHA-256, BLAKE3, bytes,
line-count i newline-complete binding. Receipt i manifest wiążą raw segment
set, raw controls, semantics/IDL, running executable oraz wszystkie JSONL.
Końcowa rewalidacja dzieje się po sync `.partial`, przed atomicznym rename.

`verify-strategy-input` jest tylko read-only adapterem authority: przyjmuje
wyłącznie `Qualified`, ponownie weryfikuje raw/semantics/executable/artifacts i
zachowuje descriptor-pinned JSONL. Nie tworzy okna, outcome'u, strategii,
eksportu ani decyzji runtime'u.

`export-window` jest odrębnym descriptor-pinned consumerem wyłącznie tego
samego `Qualified` authority. Tworzy create-new, atomiczny
`outcome_blind_windows_v2.jsonl` i `manifest_v2.json` z obserwowaną osią czasu:

```text
observation  = [birth, birth + 150000 ms)
forward gate = [birth + 150000 ms, birth + 240000 ms]
```

Wynik zawiera tylko status kompletności okna (`Complete`, truncation lub
non-exact observation). Nie oblicza state po cutoffie, returnu, PnL,
entry/exit, score, SELECTED/REST, Gatekeepera ani execution. `trajectories_v2`
zawiera wyłącznie exact trajectories; każda non-exact candidate pozostaje w
`coverage_v2`, więc dopuszczalne 999000 ppm nie rozjeżdża receiptowego
line-count authority.

Candidate bez literalnej identity `BondingCurve` nie jest przez exporter
pomijany ani heurystycznie przypisywany do birthu. `export-window` kończy się
wtedy lokalnym błędem przed utworzeniem `.partial`, ponieważ nie da się
uczciwie określić, które okno pozostaje wolne od tego non-exact evidence.

Granica forward availability pochodzi wyłącznie z tipa jednego
parent-linked chain par `BlockMeta` + unfiltered `FullBlock`, które zostały
w pełni zreconciliowane w akceptowanej kohorcie i mają zgodne finalized
`Slot`. Dla jednego slotu musi zgadzać się dokładnie jeden `parent_slot`,
`blockhash`, `parent_blockhash` i liczba wykonanych transakcji. Po pierwszym
bloku za bootstrap boundary każdy następny child musi mieć zachowaną,
zreconciliowaną parent pair i literalnie równy `parent_blockhash`; nie ma
wymagania numerycznego `slot == parent + 1`, bo skipped slot Solany jest
legalny. Czas availability chainu jest maksimum czasów ukończenia wszystkich
jego par, zaś zwracany slot jest jego tipem.

„Zgodne finalized `Slot`” oznacza parent z retained `PrimarySlotUpdate` o
statusie `Finalized`, nie unię parentów z `BlockMeta`, `Processed` lub
`Confirmed`. To wąskie doprecyzowanie authority i regresje opisuje
`ADR_8D_PROSPECTIVE_PUMP_EXACT_STATE_TAPE_V2_FINALIZED_SLOT_PARENT_AUTHORITY_20260823.md`.
Sam `PrimarySlotUpdate` pozostaje evidence canonicality, lecz nie może być
watermarkiem source completeness: nie rozróżnia skipped slotu Solany od
pominięcia lane przez providera. Cichy interwał Pump nie może sam w sobie
zamienić źródłowo pokrytego 90-sekundowego forward gate w fałszywe
`TruncatedAtRunEnd`, a naked późny Slot nie może go sztucznie otworzyć.

Cutoffy observation i forward są liczone po
`ingress_monotonic_ts_ms`; `ingress_wall_ts_ms` pozostaje tylko audytową
etykietą każdego artefaktu. Frontier otrzymuje czas późniejszego z dwóch
ingressów tworzących zreconciliowaną parę, więc coverage nie zaczyna się przed
pojawieniem się obu witnesses.

Lokalna regresja end-to-end konstruuje przez ten sam PRXTAPE2 writer mały raw
`Complete`, z realnym vendored semantics manifestem, exact `Create`, exact
`Buy`, finalnymi anchorami, prawdziwymi inner `CreateEvent`/`TradeEvent` CPI i
późniejszą pustą, lecz kompletną parą `BlockMeta` + `FullBlock`. Wartości SOL i
quote reserve są celowo różne. Następnie wywołuje publiczne `qualify` oraz
`export-window` i wymaga jednego `Complete` okna. Osobne negatywne regresje
odrzucają brak FullBlocku, równoczesne pominięcie całego parent blocku wraz z
filtered transaction, błędny child `parent_blockhash`, naked późny Slot oraz
błędne Event-CPI identity/canonical state binding. Osobne corruption fixture'y
odrzucają Account/Slot/BlockMeta projection drift przy niezmienionym,
hash-związanym retained `SubscribeUpdate`.
Ta regresja jest dowodem kompatybilności kodowych kontraktów, nie operatorowym
capture'em ani dowodem kompatybilności rzeczywistego providera.

W trakcie tej regresji potwierdzono też realny IDL shape `OptionBool`:
jednopolowy struct może zapisać primitive shorthand `"bool"` zamiast obiektu
`{"type":"bool"}`. Strict decoder obsługuje oba literalne zapisy, ale nadal
odrzuca invalid bool i trailing bytes. Nie jest to tolerancyjny fallback ani
zmiana semantyki pola.

## D4. Zakres wyłączony

Ta korekta nie:

- wykonuje V2 preflightu, capture'u, RPC, Yellowstone, GO-E ani backfillu;
- tworzy V2 raw, completion receipt, exact artifact lub strategy input;
- zapisuje endpointów, hostname'ów lub sekretów w repozytorium;
- zmienia Gatekeepera, execution, Event Bus, aktywny Seer runtime, GO-D/V1
  codec ani historyczne artefakty;
- obniża coverage/completeness threshold;
- wykonuje żadnego operatorowego exportu, outcome'ów ani live promotion.

## D5. Relacja do wcześniejszych ADR

Ten dokument **zastępuje wyłącznie D4**
`ADR_8D_PROSPECTIVE_PUMP_EXACT_STATE_TAPE_V2_CAPTURE_CONTRACT_20260821.md`,
który prawidłowo opisywał wtedy brak V2 materializera. Capture contract,
readiness overlap oraz required-lane contract z wcześniejszych ADR pozostają
niezmienione.

## D6. Przed następną operacją

Minimalny następny techniczny krok po pełnej macierzy testów i niezależnym
review tego local diffu to decyzja użytkownika o ewentualnym allowlist-only
clean commicie recordera + qualifiera. Dopiero potem, jako osobne operacje:

1. operator dostarcza finalny operator-controlled semantics manifest oraz private config;
2. local-only sealed preflight potwierdza clean release i storage dla capture;
3. jawny operator GO może uruchomić dokładnie jeden prospective V2 capture;
4. po raw `Complete` operator uruchamia offline `qualify`;
5. tylko rzeczywisty `Qualified` może uruchomić outcome-blind `export-window`;
   jego wynik nadal wymaga osobnej prerejestracji i review przed strategy
   outcomes.

Brak któregokolwiek z tych elementów pozostawia V2 `NO-GO` dla strategii.
