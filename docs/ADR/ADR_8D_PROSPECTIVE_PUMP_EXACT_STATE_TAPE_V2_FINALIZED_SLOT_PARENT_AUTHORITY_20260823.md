# ADR-8D: Prospective Pump Exact-State Tape V2 — authority parenta wyłącznie z finalized Slot

**Data:** 2026-08-23

**Status:** LOCAL VERIFICATION PASS / NEUTRAL SELF-REVIEW PASS / INDEPENDENT REVIEW PASS / ALLOWLIST-ONLY COMMIT AUTHORIZED / V2 RAW NOT CREATED / NO PROVIDER I/O

**Typ:** ADR-8D / prospective research evidence / offline fail-closed correction

> Globalny szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie był dostępny w tym
> checkoutie. Dokument zachowuje lokalną strukturę ADR-8D przyjętą przez
> wcześniejsze ADR V2.

## D0. Problem i decyzja

Niezależne review wykazało, że per-slot model łączył dwa odrębne fakty w jeden zbiór:

```text
parent z dowolnego PrimarySlotUpdate lub PrimaryBlockMeta
+
flaga „widziano Finalized” z dowolnego PrimarySlotUpdate
```

Nie dowodziło to wymaganej relacji:

```text
retained Finalized Slot.parent == BlockMeta.parent_slot
```

Prawidłowo zahashowany retained protobuf mógł zawierać `Finalized Slot { parent: None }`, gdy zgodny `BlockMeta` i `FullBlock` nazywały parent `P`. Wspólny zbiór dostawał wtedy `P` z BlockMeta. Podobnie `Processed Slot { parent: Some(P) }` i późniejszy `Finalized Slot { parent: None }` mogły stworzyć field stitching w obrębie jednego slotu.

Decyzja jest wąska i fail-closed:

```text
finalized_slot_parent_authority[slot]
  = distinct Option<parent> values wyłącznie z retained
    PrimarySlotUpdate o source_status == Finalized

BlockMeta.parent_slot
  = authority wyłącznie w niezależnym BlockMeta/full-block ledgerze

admitted BlockMeta + FullBlock pair
  requires exactly one distinct finalized-slot parent value
  && that value == Some(BlockMeta.parent_slot)
```

`None` jest zachowywane jako evidence, a nie ignorowane. Brak finalnego parenta nie może zostać naprawiony przez inny lane, inny status ani późniejszą projekcję. Powtarzające się identyczne finalized updates nie tworzą drugiej wartości parenta; sprzeczne finalized values kończą raw qualification.

Nie zmienia to P0 parent-linked `BlockMeta ↔ FullBlock` chainu, retained protobuf binding, Event-CPI, quote regime ani monotonicznej osi czasu.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```

GO-D/V1, Gatekeeper, execution, aktywny Seer runtime i strategie pozostają poza zakresem.

## D1. Implementowany kontrakt authority

`PumpExactStateSlotNodeV2` przechowuje `BTreeSet<Option<u64>> finalized_parents`. `PumpExactStateRawIndexBuilderV2` dopisuje do niego parent — również `None` — wyłącznie dla `PrimarySlotUpdate` ze statusem `Finalized`. `PrimaryBlockMeta` nie mutuje slotowego parent indexu.

Następnie te same finalized-slot-only dane są jedyną authority w obu consumerach:

1. `reconciled_full_block_frontier()` wymaga dla każdej przyjętej pary `BlockMeta + FullBlock` jednego finalnego parenta dokładnie równego `Some(BlockMeta.parent_slot)`;
2. `rooted_slots()` dopuszcza slot do canonical anchors wyłącznie pod tym samym warunkiem oraz przy lokalnie zgodnej parze BlockMeta/full-block;
3. końcowa walidacja raw odrzuca więcej niż jedną odrębną wartość parenta z finalized Slot evidence.

Parent z `Processed` lub `Confirmed` jest nadal zachowany w immutable raw source payload i jest walidowany wobec niego, lecz nie ma authority do rootedness, denominatora, frontiera ani exact-state anchors.

## D2. Wymagane regresje

Trzy przypadki przechodzą przez prawdziwy PRXTAPE2 raw writer oraz publiczne `qualify_prospective_exact_state_raw_run_v2`; żaden nie stosuje corruption helpera projekcji. Retained protobuf i raw projection zgadzają się dla wszystkich zapisanych rekordów.

1. `Finalized Slot.parent = None`, a odpowiadające `BlockMeta` i `FullBlock` deklarują `parent_slot = P`. Kwalifikacja musi failować na finalized Slot parent authority, exact output i `.partial` nie mogą powstać.
2. `Processed Slot.parent = Some(P)`, a `Finalized Slot.parent = None`; ta sama para BlockMeta/full-block deklaruje `P`. Kwalifikacja musi failować tak samo, co zakazuje field stitching między statusami Slot.
3. Dwa retained `Finalized Slot` records deklarują różne wartości parenta. Końcowa kontrola `finalized_parents` musi odrzucić raw przed frontierem, exact outputem i `.partial`.

Pozytywny skipped numeric slot i parent-linked P0 pozostają objęte istniejącą macierzą: naprawa nie wprowadza nielegalnego wymogu `slot == parent + 1`.

## D3. Zakres wyłączony

Ta korekta nie autoryzuje ani nie wykonuje:

- commita, pushu, merge ani zmiany statusu PR;
- preflightu, provider I/O, Yellowstone, RPC/GO-E, raw capture'u, backfillu lub imputacji;
- usuwania lub przenoszenia storage albo diagnostyk;
- zmian manifestu/IDL, Event-CPI, quote-regime, coverage thresholdów, monotonic window authority, Gatekeepera, runtime'u, execution lub strategii;
- publikacji outcome'ów, PnL, SELECTED/REST lub live promotion.

`.codex/active-task.md` jest local-only checkpointem i nie należy do przyszłego allowlist-only commita produktu.

## D4. Wykonana lokalna weryfikacja i neutralny self-review

Przeszły trzy publiczne regresje finalized-parent oraz pełna locked/offline macierz: `cargo fmt --all -- --check`; `cargo check --locked --offline -p seer --bin pump-exact-state-tape-v2`; semantics 9/9; materializer 20/20; pełny V2 suite 77/77; standalone CLI 1/1; `ghost-core` V2 6/6; `grpc_connection::tests` 95/95; locked/offline release build; release `--help`; `git diff --check`; `git diff --cached --check`.

Neutralny self-review ponownie sprawdził kolejność retained payload → projection validator → indeks, wyłączność `Finalized Slot` jako parent authority, brak mutacji slotowego indeksu przez BlockMeta, rootedness, konflikt finalized parentów, parent-chain P0, skipped numeric slots, brak exact/.partial publication dla trzech negatywnych fixture'ów oraz brak zmian aktywnego runtime'u, Gatekeepera i execution. Nie wykazał dalszego in-scope defectu.

Świeżo zbudowana binarka lokalna: `target/release/pump-exact-state-tape-v2`, 11,835,688 bytes, mode `0700`, SHA-256 `4245d1919b20ff0908c8ec109ed75429fb69a085d3ba6a7b8198871b5284778d`.

## D5. Następny krok

Lokalna macierz, neutralny self-review i świeże niezależne review przeszły; użytkownik wydał jawną zgodę wyłącznie na allowlist-only clean commit. Sealed preflight, storage, private operator config i jeden bounded capture pozostają odrębnymi, późniejszymi decyzjami.
