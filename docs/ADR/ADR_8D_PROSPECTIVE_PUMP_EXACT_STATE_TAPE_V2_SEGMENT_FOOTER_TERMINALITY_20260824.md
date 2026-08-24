# ADR-8D: Prospective Pump Exact-State Tape V2 — terminalność `clean_shutdown` w łańcuchu segmentów

**Data:** 2026-08-24

**Status:** IMPLEMENTED / LOCAL VALIDATION PASS / SELF-REVIEW PASS / INDEPENDENT REVIEW PASS / RAW PRESERVED / NO PROVIDER I/O

**Typ:** ADR-8D / standalone prospective V2 offline authority / fail-closed PRXTAPE3 segment-chain validation

> Globalny szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie jest dostępny w tym
> środowisku. Dokument zachowuje lokalny układ ADR-8D używany przez istniejące
> ADR-y V2.

## D0. Potwierdzony problem

Zachowany, kompletny PRXTAPE3 run
`pump-exact-state-v2-1787539185686-2720125` ma 27 atomowo opublikowanych
segmentów. Recorder zamyka każdy segment pośredni przez
`close_current(false)` podczas bounded rolloveru i zamyka wyłącznie segment
terminalny przez `close_current(true)` po planowym clean close.

To znaczy, że `clean_shutdown = false` w footerze segmentu pośredniego nie
oznacza niekompletnego runu. Jest literalnym dowodem, że segment był
kontynuowany przez następcę. Łańcuch poprzednich prefix digestów oraz complete
run receipt nadal są obowiązkowe.

Offline `scan_v2_segment()` błędnie wymagał `clean_shutdown = true` dla
każdego segmentu. W rezultacie rekwalifikacja kompletnie zachowanego raw
zatrzymywała się na pierwszym poprawnym rollover footerze przed utworzeniem
exact artifactu.

## D1. Decyzja

Kolejność `completion.segment_list` jest niezmienną authority terminalności:

```text
segment pośredni   -> footer.clean_shutdown == false
segment terminalny -> footer.clean_shutdown == true
```

Indexer oblicza oczekiwaną wartość dla każdego segmentu z jego pozycji w
zamrożonej liście receiptów i przekazuje ją do strict scannera. Scanner nadal
odrzuca każdy mismatch; flaga nie została zignorowana ani osłabiona.

Zachowane pozostają wszystkie dotychczasowe kontrole:

- storage format i indeks segmentu;
- liczba accepted records i `data_bytes`;
- footer prefix BLAKE3;
- whole-file SHA-256 i BLAKE3;
- ciągłość `previous_segment_blake3`;
- prywatne tryby authority plików;
- complete completion receipt z `clean_shutdown = true`.

## D2. Regresje

Testy obejmują:

1. terminalny footer `true` przechodzi tylko jako terminalny, a nie jako
   rollover;
2. rollover footer `false` przechodzi tylko jako segment pośredni, a nie jako
   terminalny;
3. niezgodny whole-file receipt nadal odpada fail-closed;
4. publiczny PRXTAPE3 fixture zapisany przez rzeczywisty writer z jednym
   pośrednim rolloverem (`false`) i jednym terminalnym close (`true`)
   kwalifikuje się przez publiczne API do `Qualified` bez `.partial` outputu.

## D3. Zakres wyłączony

Korekta nie zmienia:

- zachowanego raw, segmentów, manifestów ani completion receiptów;
- fizycznego PRXTAPE3 codec/storage schema;
- recordera, requestu Yellowstone, five-lane readiness, ProgramData receiptów,
  configu operatora, providerów lub credentiali;
- semantics manifestu, vendored IDL, coverage, denominatora, minimum gate,
  exact JSONL/window schema ani exportera;
- V1/GO-D, GO-E, Gatekeepera, OracleRuntime, execution ani strategii.

Nie wykonuje capture'u, preflightu, RPC, Yellowstone, GPA, snapshotu,
backfillu ani imputacji. Po review i clean commicie wolno wykonać wyłącznie
jedną nową offline rekwalifikację tego samego immutable raw; recapture nie ma
uzasadnienia.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```

## D4. Plan weryfikacji

Po korekcie wymagane są lokalne/offline:

```text
cargo fmt --all -- --check
cargo check --locked --offline -p seer --bin pump-exact-state-tape-v2
cargo test --locked --offline -p ghost-core pump_research_exact_tape_v2 --lib --no-fail-fast
cargo test --locked --offline -p seer research_exact_tape_v2 --lib --no-fail-fast
cargo test --locked --offline -p seer research_exact_tape_v2_materializer --lib --no-fail-fast
cargo test --locked --offline -p seer research_exact_tape_v2_semantics --lib --no-fail-fast
cargo test --locked --offline -p seer grpc_connection::tests --lib --no-fail-fast
cargo test --locked --offline -p seer --bin pump-exact-state-tape-v2 --no-fail-fast
cargo build --locked --offline --release -p seer --bin pump-exact-state-tape-v2
target/release/pump-exact-state-tape-v2 --help
git diff --check
git diff --cached --check
```

Następnie wymagane są neutralny self-review i świeży independent review diffu
przed allowlist-only commitem.

## D5. Wynik lokalnej weryfikacji

Po korekcie przeszły lokalnie i offline:

```text
cargo fmt --all -- --check                                      PASS
cargo check --locked --offline -p seer --bin pump-exact-state-tape-v2
                                                                  PASS
ghost-core pump_research_exact_tape_v2                            5/5 PASS
seer research_exact_tape_v2                                      76/76 PASS
seer research_exact_tape_v2_materializer                         23/23 PASS
seer research_exact_tape_v2_semantics                             9/9 PASS
seer grpc_connection::tests                                      95/95 PASS
seer standalone CLI                                                1/1 PASS
locked/offline release build                                      PASS
release --help                                                    PASS
git diff --check                                                  PASS
git diff --cached --check                                         PASS
```

Publiczna regresja `public_prxtape3_intermediate_rollover_footer_qualifies_as_a_complete_chain`
przeszła także samodzielnie. Zbudowana lokalnie, niesealed binarka ma SHA-256
`1f6448fb8fceb1cc8f4639af173f844791c593b23bf06e4a049d9f07189c6d49`,
rozmiar `11 660 008` bajtów i tryb `0700`; nie jest jeszcze operational
qualification authority.

Log macierzy jest poza repozytorium:
`/protected/operator/pump-exact-state-v2-segment-footer-validation-0d211c7-20260824.log`
(`SHA-256 d4cf0675c6eea96055938ba158b834a864365bbd641a672d6b9f20fa91cc4ecc`).
Nie wykonywano provider I/O, capture'u, preflightu, requalification ani
exportu.

## D6. Neutralny self-review

Po implementacji sprawdzono diff jako niezależny artifact:

- `expected_clean_shutdown` pochodzi wyłącznie z pozycji w sprawdzonej,
  niepustej `completion.segment_list`; nie zależy od wall clock, payloadu,
  projection ani heurystyki;
- indeks segmentu nadal musi być contiguous, a każdy child header nadal musi
  wskazywać poprzedni obliczony prefix hash — brakujący następca intermediate
  footeru nie może zostać zaakceptowany;
- scanner nadal odrzuca literalnie błędną flagę dla obu pozycji zamiast tylko
  przestawać wymagać `true`;
- test jednostkowy pokrywa obie orientacje flagi i mismatch whole-file
  receiptu, zaś test publiczny pokrywa realny writer, full raw control chain,
  qualification i atomiczny brak `.partial`;
- diff nie zmienia recordera produkcyjnego, raw schema/codec, semantics,
  requestu, configu, providerów, raw evidence ani jakiegokolwiek aktywnego
  runtime'u.

Nie znaleziono dodatkowego, popartego dowodem problemu P0/P1/P2. Następna
bramka to świeży read-only independent review przed allowlist-only commitem.

## D7. Świeży independent review

Niezależny read-only review bieżącego diffu zakończył się `PASS`
(`P0=0`, `P1=0`, `P2=0`). Reviewer potwierdził:

- rzeczywiste production rollover `close_current(false)` oraz tylko terminalne
  complete close `close_current(true)`;
- wyprowadzenie terminalności z niepustej, contiguous immutable
  `completion.segment_list`, bez zaufania payloadowi lub runtime labelowi;
- literalny mismatch rejection dla obu pozycji przy zachowaniu prefix chain,
  count i whole-file digest checks;
- real-writer, publiczną dwusegmentową regresję oraz unit rejection obu
  zamian terminal/intermediate;
- brak raw/schema/request/provider/semantics/runtime scope driftu i zgodność
  ADR z kodem.

Reviewer niezależnie uruchomił publiczną regresję, materializer `23/23`,
szerokie V2 `76/76`, format oraz diff checks. Nie edytował, nie stage'ował ani
nie commitował plików i nie wykonał provider I/O, capture'u, preflightu,
requalification ani exportu.
