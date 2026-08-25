# ADR-8D: Prospective Pump Exact-State Tape V2 — parent-chain conservation i retained-protobuf projection binding

**Data:** 2026-08-23

**Status:** SUPERSEDED IN PART / FINALIZED SLOT.PARENT SUCCESSOR INDEPENDENT REVIEW PASS / ALLOWLIST-ONLY COMMIT AUTHORIZED / V2 RAW NOT CREATED / NO PROVIDER I/O

**Typ:** ADR-8D / prospective research evidence / offline fail-closed correction

> Globalny szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie był dostępny w tym
> checkoutie. Dokument zachowuje lokalną strukturę ADR-8D przyjętą przez
> wcześniejsze ADR V2.

> **Aktualizacja 2026-08-23:** późniejsze niezależne review wykazało, że
> pierwotne wykonanie warunku `finalized Slot.parent == BlockMeta.parent_slot`
> sklejało parent z niezależnych lane'ów albo statusów Slot. Ten dokument nie
> jest już podstawą claimu local PASS dla tej części authority. Poprawkę i jej
> regresje opisuje sukcesor
> `ADR_8D_PROSPECTIVE_PUMP_EXACT_STATE_TAPE_V2_FINALIZED_SLOT_PARENT_AUTHORITY_20260823.md`.

## D0. Problem i decyzja

Następne niezależne review poprawnie wykazało, że sama bijekcja
`BlockMeta ↔ FullBlock` w tym samym slocie nie dowodzi ciągłości między
kolejnymi wyprodukowanymi blokami. Provider mógł wspólnie pominąć środkowy
blok oraz jego filtered Pump transaction; późniejszy zachowany blok nadal
zawierałby `parent_slot` wskazujący brakującego rodzica. Równość dwóch
niepełnych map Pump transaction nie mogła tego wykryć.

Review wykazało również węższą lukę authority: raw przechowuje pełny
`SubscribeUpdate`, ale projekcje `Account`, `Slot` i `BlockMeta` były
dotychczas sprawdzane wyłącznie przez hash całego payloadu. Późniejszy indeks
używał ich pól bez dowodu, że odpowiadają zachowanemu protobufowi.

Decyzja jest lokalna i fail-closed:

```text
source completeness frontier
  = tip jednego parent-linked, finalized BlockMeta + FullBlock chain

raw projection authority
  = decoded retained SubscribeUpdate

convenient raw fields
  = tylko projekcja, która musi literalnie zgadzać się z retained payloadem
```

Nie ma jeszcze realnego raw V2, dlatego nie ma migracji ani kompatybilności
wstecznej z admitted archive. V2 format i qualifier można bezpiecznie
domknąć przed pierwszym capture'em.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```

GO-D/V1 i aktywny runtime nie są przedmiotem tej decyzji.

## D1. P0 — conservation complete parent-linked chain

Offline collector nadal najpierw wymaga dla każdego zachowanego,
wyprodukowanego slotu w kohorcie dokładnej pary:

```text
BlockMeta.slot                    == FullBlock.slot
BlockMeta.parent_slot             == FullBlock.parent_slot
BlockMeta.blockhash               == FullBlock.blockhash
BlockMeta.parent_blockhash        == FullBlock.parent_blockhash
BlockMeta.executed_tx_count       == FullBlock.executed_tx_count
finalized Slot.parent             == BlockMeta.parent_slot
```

Powyższa równość jest kontraktem, lecz pierwotna implementacja nie dowodziła,
że parent pochodził z tego samego retained `Finalized Slot` recordu. Nie wolno
interpretować tej sekcji jako potwierdzenia słabszej unii parentów z
`Processed`/`Confirmed` Slot albo `BlockMeta`; tę lukę domyka sukcesor ADR.

Następnie iteruje po parach w rosnącym porządku slotów i buduje mapę już
**zreconciliowanych** parent hashes. Pierwsza przyjęta para może wskazywać
rodzica na bootstrap boundary albo przed nim, ponieważ bootstrap nie zachowuje
historycznego pełnego blockhash chain. Każda kolejna para musi jednak:

1. mieć `parent_slot < child.slot`;
2. mieć `parent_slot > bootstrap_boundary`;
3. znaleźć już zreconciliowaną parę tego rodzica;
4. mieć `child.parent_blockhash == parent.blockhash`;
5. rozszerzać aktualny tip tego samego complete chain, a nie tworzyć drugi
   lokalnie poprawny fork/root.

Nie ma reguły `slot == parent_slot + 1`. W Solanie sloty numeryczne mogą być
skipped; prawidłowe `103 -> 105` jest legalne, jeśli blok `105` literalnie
wskazuje zachowany parent `103` i jego blockhash. Natomiast `105 -> 104`, gdy
104 jest ponad boundary, ale nie istnieje w raw ledgerze, kończy raw
qualification przed utworzeniem exact outputu.

Frontier jest teraz tipem pełnego chainu, nie parą o największym samotnym
ingressie. Jego monotoniczny czas availability to maksimum czasów ukończenia
wszystkich par chainu: dopiero wtedy lokalnie istnieją wszystkie witnesses
konieczne do dowodu tego tipa. Wall time pozostaje wyłącznie audytową etykietą.

## D2. P1 — wiązanie projekcji z pełnym retained payloadem

Po `BLAKE3(source_payload)` collector dekoduje zachowany kompletny
`SubscribeUpdate`, wymaga właściwego wariantu protobufu i porównuje go z raw
projekcją zanim indeks lub canonicality logic odczyta którekolwiek z jej pól.

| Wariant raw | Pola literalnie porównywane z retained protobufem |
| --- | --- |
| `PumpOwnedAccountUpdate` | `slot`, `is_startup`, `pubkey`, `owner`, pełne raw `data`, `write_version`, opcjonalny `txn_signature`, `evidence_class` |
| `PrimarySlotUpdate` | `slot`, `parent`, `status` |
| `PrimaryBlockMeta` | `slot`, `parent_slot`, `blockhash`, `parent_blockhash`, `executed_transaction_count`, `block_time` |

Dla accountu validator dodatkowo wymaga, aby source owner był pinned Pump
programem. `evidence_class` korzysta ze wspólnego klasyfikatora recordera i
qualifiera, więc canonical Global/BondingCurve/Other nie mogą stać się
dwoma niezależnymi źródłami authority.

Ingress wall/monotonic timestamp nie jest polem `SubscribeUpdate`; nadal jest
kontrolowany przez istniejący raw ingress contract. P1 domyka wyłącznie pola,
które są duplikowane jako projekcja pól zachowanego protobufu.

## D3. Regresje decyzji

Publiczne fixture'y przechodzą przez ten sam PRXTAPE2 writer i publiczne
`qualify`/`export-window`. Obejmują:

1. `103 -> 104 -> 105`, po czym usuwają cały produkowany blok 104 ze Slot,
   BlockMeta, FullBlock, filtered transaction i account lanes, zostawiając
   `105.parent_slot = 104`: qualification jest błędem, exact output nie
   powstaje;
2. zachowany parent i dziecko, lecz zgodnie sfałszowany po obu lane'ach
   dziecka `parent_blockhash`: raw qualification failuje przed publikacją;
3. prawidłowy skipped numeric slot `103 -> 105 -> 106`: kwalifikacja i
   outcome-blind export przechodzą, bez wymagania slotu 104;
4. frontier zwraca tip `105`, a nie lokalną parę `104`, gdy kompletna para
   rodzica dociera później niż już zachowana para dziecka; jego availability
   time jest późniejszym czasem wymaganym przez cały chain;
5. zachowany protobuf Account z celowo zmienionym jedynie raw
   `evidence_class`;
6. zachowany protobuf Slot z celowo zmienionym jedynie projected `parent`;
7. zachowany protobuf BlockMeta z celowo zmienionym jedynie projected
   `blockhash`.

W trzech ostatnich przypadkach raw record jest ponownie poprawnie framed i
receipted, a source payload oraz jego BLAKE3 pozostają niezmienione. Błąd musi
więc pochodzić z semantycznej projection binding, nie z outer frame/hash.

## D4. Zakres wyłączony

Ta korekta nie wykonuje ani nie autoryzuje:

- commita, pushu, merge ani zmiany statusu PR;
- preflightu, provider I/O, Yellowstone, RPC/GO-E, raw capture'u, backfillu
  ani imputacji;
- usuwania/przenoszenia diagnostyk i artefaktów;
- zmian Gatekeepera, execution, Event Bus, aktywnego Seer runtime'u, strategii
  lub operator configuration;
- zmian Event-CPI/quote-regime authority lub monotonic time contractu;
- outcome'ów, PnL, SELECTED/REST i live promotion.

`.codex/active-task.md` pozostaje local-only checkpointem poza przyszłym
allowlist-only commitem produktu.

## D5. Historyczna lokalna weryfikacja — zastąpiona w zakresie finalized Slot.parent

Poniższa macierz i self-review nadal dokumentują P0 parent-chain oraz retained-protobuf projection binding, ale nie potwierdzają pełnego local code authority.
Późniejsze niezależne review wykazało, że `Finalized Slot { parent: None }` mógł zostać pozornie naprawiony przez `BlockMeta`, a parent z `Processed Slot` mógł zostać sklejony z finality innego recordu.
Sukcesor przeszedł lokalną macierz, neutralny self-review oraz świeże niezależne review. Użytkownik autoryzował wyłącznie allowlist-only clean commit; sealed preflight i capture nadal nie są autoryzowane.

Po ostatniej zmianie źródłowej przeprowadzono pełną locked/offline macierz:

- `cargo fmt --all -- --check` — PASS;
- `cargo check --locked --offline -p seer --bin pump-exact-state-tape-v2` —
  PASS;
- semantics — 9/9 PASS;
- materializer — 20/20 PASS, w tym test delayed-parent evidence → chain tip;
- pełny V2 suite — 74/74 PASS, w tym trzy wymagane P0 i trzy wymagane P1
  regresje;
- standalone CLI — 1/1 PASS;
- `ghost-core` V2 — 6/6 PASS;
- `grpc_connection::tests` — 95/95 PASS;
- locked/offline release build — PASS;
- release `--help` — PASS i opisuje finalized parent-linked chain;
- `git diff --check` oraz `git diff --cached --check` — PASS.

Świeży self-review kompletnego dirty diffu sprawdził:

1. local BlockMeta/full-block pairing, parent lookup, parent blockhash i brak
   nielegalnej numerycznej continuity rule;
2. wspólne omission całego pośredniego bloku i brak exact publication;
3. Account/Slot/BlockMeta projection binding do retained protobufu przed
   indexem/rootedness;
4. niezmienione Event-CPI/quote-regime oraz monotonic-window authority;
5. brak denominator shrink, unscoped candidate drop, publication dla
   `Blocked` oraz zmian aktywnego runtime'u/Gatekeepera/execution.

Nie znaleziono dalszego in-scope defectu. Zielony test i local self-review nie
są jednak upoważnieniem do commita: clean allowlist-only commit wymaga
osobnej, jawnej zgody użytkownika.

Sealed preflight, storage cleanup, prywatny operator config i pierwszy realny
capture pozostają osobnymi decyzjami operatora po clean commit i niezależnym
zewnętrznym PASS.
