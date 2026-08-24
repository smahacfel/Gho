# ADR-8D: Prospective Pump Exact-State Tape V2 — semantyczna tożsamość ProgramData receipt w offline validatorze

**Data:** 2026-08-24

**Status:** LOCAL VALIDATION PASS / SELF-REVIEW PASS / INDEPENDENT REVIEW PASS / ALLOWLIST-ONLY COMMIT CREATED / NO REQUALIFICATION YET

**Typ:** ADR-8D / standalone prospective V2 offline authority / fail-closed ProgramData receipt consistency

> Globalny szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie jest dostępny w tym
> środowisku. Dokument zachowuje lokalny układ ADR-8D używany przez istniejące
> ADR-y V2.

## D0. Potwierdzony problem

Zachowany PRXTAPE3 run
`pump-exact-state-v2-1787539185686-2720125` zakończył capture jako `complete`.
Recorder potwierdził niezmienność ProgramData między finalized receiptami przez
`program_data_receipts_match_v2()` i zapisał `program_data_unchanged = true`.

Startowy oraz końcowy receipt mają identyczne immutable authority fields:

- Pump Program pubkey i owner;
- ProgramData pubkey i owner;
- algorithm oraz BLAKE3 danych ProgramData;
- deployment slot;
- finalized commitment.

Różni się wyłącznie `observed_context_slot`: końcowy finalized RPC odczyt
powstaje później niż startowy. Pole jest audytową etykietą punktu obserwacji,
nie częścią immutable tożsamości programu.

Mimo tego offline `validate_v2_controls()` wymagał whole-struct equality
`program_data_at_completion == program_data_at_start`. Obejmowało to także
`observed_context_slot` i odrzucało prawidłowy raw przed utworzeniem exact
artifactu.

## D1. Decyzja

Jedyna definicja immutable ProgramData authority pozostaje w
`program_data_receipts_match_v2()` i jest udostępniona `pub(crate)` dla
offline materializera.

Validator nadal wymaga dosłownego skopiowania startowego receiptu do completion
receiptu:

```text
completion.program_data_at_start == start.program_data_at_start
```

Natomiast independently observed completion receipt jest ponownie sprawdzany
wspólnym semanticznym comparatorem:

```text
program_data_receipts_match_v2(
    &start.program_data_at_start,
    completion.program_data_at_completion,
)
```

Validator nadal wymaga `program_data_unchanged == true`, lecz nie ufa samemu
booleanowi: sam przelicza semanticzną zgodność receiptów.

Nie zmienia się schema raw/receiptów. `observed_context_slot` pozostaje
zachowany do audytu, ale nie może udawać zmiany immutable ProgramData authority.

## D2. Regresje

Publiczne PRXTAPE3 fixturey pokrywają:

1. późniejszy `observed_context_slot` przy identycznej ProgramData authority
   → raw controls przechodzą i fixture kwalifikuje się;
2. osobną zmianę każdego pola semantic identity:
   Pump Program pubkey, Pump Program owner, ProgramData pubkey, ProgramData
   owner, hash algorithm, ProgramData BLAKE3, deployment slot oraz commitment
   → raw authority odrzucone przed finalnym lub `.partial` exact outputem;
3. rozjazd `completion.program_data_at_start`, brak
   `program_data_at_completion` oraz `program_data_unchanged = false`
   → raw authority odrzucone fail-closed.

## D3. Zakres wyłączony

Korekta nie zmienia:

- zachowanego raw PRXTAPE3, segmentów, start manifestu ani completion receiptu;
- recordera, Base64 ProgramData RPC readera, Yellowstone requestu, five-lane
  readiness, full-block lane, configu operatora, providerów lub credentiali;
- semantics manifestu, vendored IDL, exact JSONL/window schema, coverage,
  denominatora albo minimum qualification gate;
- V1/GO-D, GO-E, Gatekeepera, OracleRuntime, execution ani strategii.

Nie uruchamia requalification, exportu, capture'u, sealed capture preflightu,
RPC, Yellowstone, GPA, snapshotu, backfillu ani imputacji.

Po independent review i czystym allowlist-only commicie późniejsza osobna zgoda
może uruchomić dokładnie jedną create-new offline requalification zachowanego
raw. Nie ma podstaw do recapture'u.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```

## D4. Plan weryfikacji

Po korekcie wymagane są co najmniej:

```text
cargo fmt --all -- --check
cargo check --locked --offline -p seer --bin pump-exact-state-tape-v2
cargo test --locked --offline -p seer research_exact_tape_v2 --lib --no-fail-fast
cargo test --locked --offline -p seer research_exact_tape_v2_materializer --lib --no-fail-fast
cargo test --locked --offline -p seer research_exact_tape_v2_semantics --lib --no-fail-fast
cargo test --locked --offline -p seer --bin pump-exact-state-tape-v2 --no-fail-fast
cargo test --locked --offline -p ghost-core pump_research_exact_tape_v2 --lib --no-fail-fast
cargo test --locked --offline -p seer grpc_connection::tests --lib --no-fail-fast
cargo build --locked --offline --release -p seer --bin pump-exact-state-tape-v2
target/release/pump-exact-state-tape-v2 --help
git diff --check
git diff --cached --check
```

Wszystkie kontrole są lokalne/offline. Nie są requalification, exportem,
preflightem ani provider I/O.

## D5. Wynik lokalnej weryfikacji

Po implementacji przeszły lokalnie i offline:

```text
cargo fmt --all -- --check                                      PASS
cargo check --locked --offline -p seer --bin pump-exact-state-tape-v2
                                                                  PASS
ghost-core pump_research_exact_tape_v2                            5/5 PASS
seer research_exact_tape_v2                                      75/75 PASS
seer research_exact_tape_v2_materializer                         23/23 PASS
seer research_exact_tape_v2_semantics                             9/9 PASS
seer grpc_connection::tests                                      95/95 PASS
seer standalone CLI                                                1/1 PASS
locked/offline release build                                      PASS
release --help                                                    PASS
git diff --check                                                  PASS
git diff --cached --check                                         PASS
```

Trzy nowe publiczne regresje przeszły także osobno. Zbudowana lokalnie,
niesealed binarka ma SHA-256
`85391bde64f1a112f35f64aa48c29d0699bcd7e87509be8e6ac50365d79e222c`,
rozmiar `11 659 704` bajty i tryb `0700`. To jest wyłącznie artefakt
walidacyjny; nie jest replacement capture authority.

Logi lokalnej macierzy są poza repozytorium w
`/tmp/codex-v2-programdata-validator-qBk1jX`. Nie wykonywano provider I/O,
capture'u, preflightu, requalification ani exportu.

## D6. Neutralny self-review

Po lokalnej macierzy sprawdzono ponownie cały dirty diff i kontrakt authority:

- recorder i validator wywołują tę samą funkcję comparatora; nie pozostał
  whole-struct comparator completion receiptu;
- copied start receipt nadal wymaga dosłownej równości z start manifestem;
- `program_data_unchanged` pozostaje literalną fail-closed bramką i jest
  uzupełniony, a nie zastąpiony, niezależnym semanticznym porównaniem;
- osiem porównywanych pól odpowiada wszystkim polom `PumpProgramDataReceiptV1`
  poza celowo audytowym `observed_context_slot`;
- publiczne regresje sprawdzają zarówno dopuszczalny późniejszy observation
  context, jak i wszystkie semantyczne/control drifty przed finalnym oraz
  `.partial` outputem;
- diff nie dotyka raw, receiptów, PRXTAPE3 codec/schema, requestu Yellowstone,
  providerów, configu, semantics/IDL, GPA/backfillu ani aktywnego runtime'u.

Nie znaleziono dodatkowego, popartego dowodem problemu P0/P1/P2. Następna
bramka to osobno autoryzowana offline requalification; nie jest nią capture.

## D7. Świeży independent review

Niezależny, read-only review bieżącego dirty diffu zakończył się
`PASS (P0=0, P1=0, P2=0)`. Potwierdził w szczególności:

- jedyne dziewięć pól `PumpProgramDataReceiptV1`, z których osiem immutable
  pól jest porównywanych, a `observed_context_slot` jest jedyną audit-only
  etykietą;
- wspólny comparator w recorderze i materializerze, literalny copied-start
  binding oraz niezależną fail-closed rewalidację completion receipt;
- publiczne pozytywne i negatywne fixturey przed utworzeniem finalnego lub
  `.partial` exact outputu;
- brak PRXTAPE3/request/config/provider/semantics/runtime driftu.

Reviewer niezależnie uruchomił trzy nowe regresje, materializer `23/23`,
szeroki V2 `75/75`, `cargo fmt --all -- --check` oraz kontrolę diffów. Nie
edytował, nie stage'ował ani nie commitował plików, nie wykonał provider I/O,
nie odczytał `.env`/sekretów i nie uruchomił requalification zachowanego raw.
