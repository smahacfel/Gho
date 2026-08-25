# ADR-8D: Prospective Pump Exact-State Tape V2 — oddzielny kontrakt capture

**Data:** 2026-08-21

**Status:** HISTORICAL / SUPERSEDED IN PART BY PRXTAPE3 STREAM-ONLY V1.1 / REAL SOURCE CAPTURE NOT STARTED

**Task:** `PROSPECTIVE_PUMP_EXACT_STATE_TAPE_V2_CAPTURE_CONTRACT`

> **Zakresowe zastąpienie, 2026-08-23:**
> `ADR_8D_PROSPECTIVE_PUMP_EXACT_STATE_TAPE_V2_STREAM_ONLY_V1_1_READINESS_20260823.md`
> zastępuje ten ADR **wyłącznie** w obszarze all-owner Pump account subscription
> oraz bootstrapu `getProgramAccounts(Pump)`. PRXTAPE3 nie wykonuje snapshotu
> kont ani RPC backfillu: account lane obejmuje tylko BondingCurve i canonical
> Global ze streamu Yellowstone. Pozostałe historyczne granice capture'u z tego
> ADR-u nie są przez tę adnotację zmieniane.

## D0. Problem i rozdzielenie authority

Historyczny GO-D pozostaje zamrożonym, hash-zweryfikowanym źródłem dla
historycznych eksperymentów:

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```

Nie wolno go zmieniać, dopisywać do niego, wykonywać RPC backfillu ani używać
nowego capture jako pozornej naprawy historycznego eksperymentu.

Diagnoza exact-state pokazała natomiast, że historyczny format V1 nie zachował
wszystkich historycznych parent AccountMeta/vector contracts potrzebnych do
konserwatywnej, pełnej kwalifikacji exact-state. Kontynuowanie zgadywania tych
kontraktów byłoby słabsze epistemicznie niż zebranie osobnego, prospektywnego
źródła o szerszym raw contract.

Decyzja: powstaje **oddzielny Tape V2**, przeznaczony wyłącznie dla przyszłej
prospektywnej kohorty. V2 nie jest wariantem materializera GO-D i nie zmienia
frozen raw V1.

## D1. Nowy surowy kontrakt V2

V2 ma własny magic/version/codec/segment chain w `ghost-core` i zapisuje tylko
create-new runy:

```text
<private V2 output root>/pump-exact-state-v2-<id>/raw-v2/
  run_start_manifest_v2.json
  segment_00000.bin ...
  run_completion_receipt_v2.json
```

Nie ma ścieżki, w której V2 zapisuje do `raw/`, `raw-v2/` istniejącego runu,
do katalogu wewnątrz historycznej authority albo do aktywnego Seera.

Każda źródłowa wiadomość jest zachowana jako decoded-protobuf,
schema-lossless evidence. Nie jest to deklaracja wire-byte losslessness gRPC
ani niezależnego audytu providera.

V2 source contract obejmuje jednocześnie:

1. wszystkie AccountUpdate należące do Pump program owner, bez filtru po
   discriminatorze; offline klasyfikacja może wskazać CanonicalBondingCurve,
   CanonicalGlobal albo `OtherPumpOwned`, lecz nie może przed capture ukryć
   nieznanego konta;
2. transakcje account-include Pump, łącznie ze successful i failed;
3. niefiltrowane full blocks z `include_transactions = true`, zachowane jako
   started/chunks/completed z SHA-256 i BLAKE3 payloadu;
4. Slot i BlockMeta potrzebne do późniejszego rooted/canonical selection;
5. jeden finalized `getProgramAccounts(Pump)` bootstrap źródłowego providera,
   wraz z literalnymi response bytes, wszystkimi Pump-owned accounts i
   account-set digestem;
6. sealed `ProspectiveReadinessBoundary` z finalized context slot `S`,
   pierwszymi admitted slotami wszystkich wymaganych lane oraz wyliczonym
   `source_readiness_slot`.

Recorder może zapisać readiness boundary dopiero, gdy każdy wymagany lane
(transaction, Pump-owned account, Slot, BlockMeta, full block) dostarczył co
najmniej jeden admitted slot, a finalizowany bootstrap spełnia literalnie:

```text
source_readiness_slot = max(first_lane_slots)
finalized_context_slot >= source_readiness_slot
```

Starsza odpowiedź GPA jest retryowana wyłącznie w ramach istniejącego,
hash-pinned bootstrap timeoutu; brak overlapu daje `Incomplete`. Przyszła
kohorta jest dopuszczalna wyłącznie dla slotów **ściśle większych niż `S`**.
Niefiltrowane full blocks są in-tape evidence do późniejszego porównania
wszystkich invocations Pump z filtrowaną lane; nie są drugim providorem ani
substytutem zewnętrznego provider audit.

## D2. Twarde granice capture

V2 jest standalone binary `pump-exact-state-tape-v2`, niedostępny z
`SeerConfig`, `connect_geyser()`, Event Busa, Gatekeepera ani execution.

Operator config jest strict `deny_unknown_fields`, hash-pinned w preflighcie i
jest odczytywany wyłącznie jako private (`0600`), regular, non-symlink file.
Zawiera jawnie:

- source queue item capacity oraz serialized-protobuf-byte capacity;
- finalized bootstrap timeout i response ceiling;
- finite `cohort_capture_wall_ms`;
- `max_raw_bytes` dla jednego runu;
- `min_free_bytes`, które musi pozostać po stronie filesystemu;
- private, istniejący output root poza każdą raw authority.

Preflight, jeszcze przed provider I/O, wymaga:

```text
available filesystem bytes >= max_raw_bytes + min_free_bytes + 64 MiB metadata allowance
```

Writer sprawdza runtime storage floor okresowo, odmawia utworzenia segmentu,
jeżeli raw byte budget nie pomieści nawet nagłówka, oraz odmawia każdego
dalszego zapisu po przekroczeniu `max_raw_bytes`. Taka sytuacja daje
`Incomplete`, a nie skrócony „Complete”.

`segment_max_bytes` jest limitem finalnego pliku, nie limitem prefixu przed
footerem: writer rezerwuje miejsce na terminalny frozen footer przed dopisaniem
rekordu i odrzuca run, gdy footer nie może zmieścić się w fizycznym limicie.

Source receive path najpierw zamienia decoded protobuf w jeden deterministyczny
payload bytes dla kolejki; kolejka nie przechowuje potencjalnie
over-allocated decoded object graph. Następnie używa bounded `try_send` i
niezależnego byte accounting. Rekord ponad byte budget, data/control lane failure, gap, source worker error,
writer error, bootstrap failure, **każde przerwanie już ustanowionego streamu**
albo stream epoch change kończą capture fail-closed. V2 zgłasza interruption
do sinku zanim shared connector wejdzie w retry: nawet SIGINT między
rozłączeniem a udanym reconnectem nie może zamienić przerwy w czysty stop.
Zmiana epoch nie może połączyć reconnectu w jedną prospektywną continuity
claim.

Writer drenuje skończony batch source/control na jedną turę i następnie daje
bootstrapowi własną turę. Ciągły ruch Yellowstone nie może zatem zagłodzić
zapisania finalized snapshotu albo readiness boundary.

Normalny raw-complete stop jest możliwy tylko po:

```text
operator SIGINT
  albo
hash-pinned cohort wall deadline
```

Nieoczekiwane zakończenie source tasku nie jest normalnym stopem.

## D3. Authority i operator preflight

Przed capture wymagany jest create-new local-only preflight z:

- clean worktree i exact Git commit;
- release bootstrap binary;
- świeżym `cargo build --locked --offline --release -p seer --bin
  pump-exact-state-tape-v2` wykonanym w izolowanym target directory;
- copied fresh release binary, build logiem i fresh-build receiptem wewnątrz
  prywatnego, create-new bundle;
- kernel-bound `/proc/self/exe` digest, a nie mutable executable pathname;
- config SHA-256/BLAKE3;
- complete concrete V2 SubscribeRequest fingerprint;
- V2 source semantics label;
- finite storage/time contract.

Preflight jest lokalną bramką provenance, nie hermetycznym systemem build
reproducibility. Jego literalna deklaracja brzmi: **clean-commit, locked,
offline fresh release build with isolated target and operator credentials
removed**. Nie twierdzi, że snapshotuje pełne środowisko hosta albo że sam
Cargo build jest niezależnym audytem.

Capture musi być uruchamiany dokładnie z copied release binary znajdującego
się w bundle. Przed jakimkolwiek provider I/O proces sprawdza SHA-256/BLAKE3
kernel-bound `/proc/self/exe` względem digestu copied binary, a także ponownie
czyta i wiąże bounded/no-follow: preflight receipt, copied binary, build log
i build receipt. Nie odczytuje później `target/release`, bieżącego Git HEAD
ani mutable executable pathname jako capture authority.

Receipt nie serializuje endpointu, hostname ani credentialu. Config template
jest jawnie nieoperacyjny (`.invalid`) i nie zawiera sekretów.

Po capture raw completion receipt wymaga między innymi: sealed bootstrap z
udowodnionym overlapem, braku gapów, jednolitego stream epoch, drained source
byte queue, unchanged ProgramData, unchanged running executable, normalnego
stopu, runtime storage floor oraz byte-budget compliance. Wymaga również
deterministycznego, writer-owned census: dodatniej liczby transaction,
account, Slot, BlockMeta i full-block payloadów, `full_blocks_started ==
full_blocks_completed`, zera orphaned/incomplete chunks oraz pełnej
reconciliation payloadów. Semantycznie cichy albo częściowo subskrybowany
stream daje `Incomplete` i unclean terminal footer, nawet gdy transport
technicznie zakończył się bez błędu.

`Complete` znaczy wyłącznie **raw V2 capture complete**. Nie znaczy
`ExactStateCapability = Qualified`, nie otwiera strategii, exportu ani live
promotion.

## D4. Historyczny zakres wyłączony

W chwili utworzenia tego ADR V2 materializer/qualifier nie istniał. To
historyczne ograniczenie zostało zastąpione wyłącznie przez
`ADR_8D_PROSPECTIVE_PUMP_EXACT_STATE_TAPE_V2_QUALIFICATION_AUTHORITY_20260822.md`.
Nowy qualifier pozostaje offline-only i nie zmienia żadnego z pozostałych
granic capture opisanych tutaj.

Ówczesna korekta capture nie:

- zmienia GO-D raw V1, jego segmentów, hashy, manifestów ani receipts;
- wykonuje RPC, Yellowstone, GO-E, backfill, capture lub strategy run;
- wprowadza V2 do aktywnego Ghost runtime;
- zmienia Gatekeepera, MFS, execution, parsera runtime ani existing V1 codec;
- obniża przyszłych qualification gates.

Osobny operator GO jest nadal wymagany przed source compatibility probe,
preflight i jednym rzeczywistym V2 capture. Dopiero po jego immutable raw
receipt powstanie osobny offline V2 materializer/qualification review.

## D5. Local verification i rollback

Lokalne regresje obejmują:

- all-Pump-owned account retention oraz non-Pump owner rejection;
- source full-block chunks i identyczny capture sequence przez writer;
- sealed finalized bootstrap/readiness boundary;
- epoch-change fail-closed;
- established-stream interruption before reconnect fail-closed;
- item i serialized-protobuf-byte queue budget;
- raw byte budget bez ubocznego `.partial` segmentu;
- FIFO authority-file substitution;
- strict output isolation oraz storage-budget arithmetic;
- independent V2 codec framing/header/footer chain.

Rollback to niewykonywanie V2 preflight/capture i niepowstawanie V2 raw runu.
Nie obejmuje resetu GO-D, nie usuwa historycznych artefaktów i nie przywraca
prób dopowiadania brakujących historycznych vectorów.
