# ADR-8D: Pump Research G.4 — regresje korupcji epoch indexera i rotacji writera

**Data:** 2026-08-17

**Status:** IMPLEMENTED / TEST-ONLY / NO PROVIDER I/O

**Task:** `PUMP_RESEARCH_G4_EPOCH_INDEXER_CORRUPTION_REGRESSIONS`

## D0. Problem

Implementacja Amendment G.4 poprawnie egzekwowała trzy kontrakty:

1. epoch nagłówków segmentów nie może się cofnąć;
2. epoch każdego source/gap recordu musi odpowiadać epoch nagłówka segmentu;
3. zmiana epoch w writerze musi zamknąć stary segment i otworzyć nowy.

Dotychczasowy corpus testował selektor qualification range, lecz nie posiadał
bezpośrednich negatywnych fixture'ów fizycznego indexera ani bezpośredniego
testu rotacji writera. Usunięcie walidacji z indexera mogłoby więc nie zostać
wykryte przez testy czystego selektora.

## D1. Decyzja: kryptograficznie poprawne fixture'y indexera

Dodano testowy builder pełnego raw runu V1, który tworzy:

- poprawny frozen segment header;
- poprawne framed raw records;
- poprawny prefix BLAKE3;
- poprawny terminal footer;
- poprawny whole-file SHA-256 i BLAKE3;
- poprawny `PumpResearchSegmentReceiptV1`;
- poprawny start manifest i completion receipt;
- ciągłą source capture sequence i zgodne record/gap accounting.

Builder nie zmienia produkcyjnego codecu ani formatu. Używa publicznego,
zamrożonego `PumpResearchRawCodecV1`, dzięki czemu negatywny test dociera do
kontraktu epoch po przejściu framingu, chainu, footerów i digestów.

## D2. Regresje

### `raw_index_rejects_segment_stream_epoch_regression`

Test buduje dwa poprawne segmenty:

```text
segment_00000.header.stream_epoch = 2
segment_00001.header.stream_epoch = 1
```

Oba segmenty mają poprawne hashe, footery, previous-prefix chain i completion
receipt. Indexer musi zakończyć się dokładnie błędem regresji `2 -> 1`, a nie
błędem digestu albo footera.

### `raw_index_rejects_record_epoch_different_from_segment_header`

Test tabelaryczny obejmuje:

- `PrimaryTransaction`;
- `PrimaryAccountUpdate`;
- `PrimarySlotUpdate`;
- `PrimaryBlockMeta`;
- `CoverageGap`.

Dla każdego wariantu najpierw tworzony jest control run z epoch rekordu zgodnym
z nagłówkiem i pełny index musi przejść. Następnie drugi, kryptograficznie
poprawny run zachowuje header epoch `1`, lecz zapisuje badany record z epoch
`2`. Jedynym akceptowanym wynikiem jest fail na record/header epoch mismatch.

### `writer_rotates_segment_when_stream_epoch_changes`

Test uruchamia rzeczywisty bounded `PumpResearchCaptureCoordinatorV1`, podaje
transaction epoch `1`, następnie epoch `2`, kończy source i sprawdza:

```text
segment_00000.header.stream_epoch = 1
segment_00000.footer.clean_shutdown = false

segment_00001.header.stream_epoch = 2
segment_00001.footer.clean_shutdown = true
```

Dodatkowo sprawdzane są segment indices, accepted counts, prefix chain,
record/header epoch parity i brak obu `.partial`.

## D3. Wpływ i granice

Zmiana jest wyłącznie testowa. Nie zmieniono:

- selektora qualification range;
- indexera produkcyjnego;
- writera produkcyjnego;
- frozen raw V1 layout lub codecu;
- GO-D raw ani historycznego GO-E0 receiptu;
- configu, provider timeoutów, retry lub concurrency;
- active Seer, Gatekeepera, MFS, execution ani strategii.

Nie wykonano Yellowstone, zewnętrznego RPC, GO-E0, `certify`, exact writera ani
exportu. Następny provider probe i combined qualification nadal wymagają
osobnych decyzji.

## D4. Weryfikacja

Pierwsze targeted wykonania po dodaniu regresji:

```text
research_tape_materializer                              28 passed
writer_rotates_segment_when_stream_epoch_changes        1 passed
```

Końcowa bramka obejmuje również pełny filtr `research_tape`, frozen
`ghost-core::pump_research_tape`, format check i oba Git whitespace checks.
