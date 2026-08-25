# ADR-8D: Prospective Pump Exact-State Tape V2 — Borsh allocation padding i wymagany prefiks instrukcji

**Data:** 2026-08-24

**Status:** IMPLEMENTED LOCALLY / LOCAL OFFLINE VALIDATION PASS / RAW PRXTAPE3 IMMUTABLE

**Typ:** ADR-8D / standalone prospective V2 offline qualifier / fail-closed evidence interpretation

> Globalny szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie jest dostępny w tym
> środowisku. Dokument zachowuje lokalny układ ADR-8D używany przez istniejące
> ADR-y V2.

## D0. Potwierdzony problem

Zachowany, kompletny PRXTAPE3 został poprawnie zapisany i przeszedł raw
integrity gate, lecz pierwsza offline qualification zakończyła się
`Blocked`. Read-only census immutable raw wykazał dwa błędy interpretacji
w kwalifikatorze, nie błąd streamu ani capture'u.

1. `BondingCurve` ma pinned Borsh serialization o długości 115 bajtów, ale
   Yellowstone zachował legalne alokacje 150, 151 i 256 bajtów. Każdy bajt za
   115-bajtowym prefixem był literalnym zerem. Poprzedni validator wymagał,
   aby pełna długość alokacji równała się 115, więc fałszywie zliczał legalne
   canonical anchors jako account decode failures.

2. Pinned IDL opisuje obowiązkowy uporządkowany prefiks account metas, ale
   official Pump IDL dokumentuje również wariantowe Anchor
   `remaining_accounts`. Ponadto retained Yellowstone `InnerInstruction`
   przechowuje indeksy kont, lecz nie local CPI signer/writable privileges.
   Poprzednia equality całego vectora i message-wide flags fałszywie
   odrzucała legalne trailing accounts oraz inner `invoke_signed` CPI.

Raw, segmenty, start manifest, completion receipt i poprzedni diagnostic
exact artifact pozostają immutable. Nie ma zgody na ich zmianę ani na
recapture z powodu błędu offline interpretacji.

## D1. Decyzja

### Borsh allocation padding

Account decoder wybiera dokładnie jeden pinned serialized extent:

- literalna długość z `allowed_serialized_bytes` zachowuje dotychczasowy
  exact-envelope contract;
- większa raw allocation jest dopuszczalna tylko wtedy, gdy dokładnie jeden
  pinned extent nie przekracza długości raw i cały suffix za nim jest zerowy;
- zero compatible extents, niezerowy suffix albo więcej niż jeden compatible
  extent są błędami fail-closed.

Następnie pełny Borsh decode i fixed field reads nadal dotyczą wyłącznie
pinned prefixu z vendored IDL. Żaden suffix nie staje się state authority.

### Pump instruction account vectors

Vetted IDL `accounts` jest teraz obowiązkowym uporządkowanym prefiksem, nie
pełnym i wyłącznym vectorem source transaction.

- prefix krótszy niż contract, zły static pubkey, duplicate named role lub
  nierozwiązywalny index nadal są błędami;
- outer Pump instruction nadal wymaga każdego IDL signer/writable privilege,
  ale transaction-message flags są używane jako lower bound: observed
  privilege superset jest legalny;
- inner Pump CPI nie dostaje wymyślonej authority signer/writable, bo protobuf
  jej nie zachowuje; nadal waliduje kolejność wymaganego prefixu oraz static
  addresses;
- każdy trailing `remaining_account` musi rozwiązać się do retained message,
  lecz nie otrzymuje named role ani canonical exact-state authority.

Strict `Anchor emit_cpi!` transport pozostaje osobnym contractem: nadal
wymaga dokładnie jednego `__event_authority` account i nie akceptuje trailing
superset.

## D2. Regresje

Dodano lokalne regresje oparte na realnym vendored Pump IDL/manifest:

- 115-byte BondingCurve oraz zero-filled 150/151/256-byte allocations
  dekodują do tego samego state; jeden niezerowy bajt suffixu jest odrzucany;
- outer `buy` akceptuje valid remaining account i privilege superset, ale
  odrzuca zły static prefix, skrócony prefix i nieistniejący trailing index;
- inner `buy` bez message-level user signer może przejść jedynie jako inner
  CPI, a identyczny brak nadal odpada dla outer instruction;
- publiczny PRXTAPE3 Complete -> Qualified -> outcome-blind export fixture
  używa 151-bajtowego zero-padded BondingCurve przez rzeczywisty writer i
  publiczny qualifier.
- osobny publiczny PRXTAPE3 fixture przechodzi przez writer i qualifier z
  resolved trailing Pump `remaining_account`; nie może on stworzyć nowego
  named role ani obniżyć denominatora.

## D3. Zakres wyłączony

Korekta nie zmienia:

- PRXTAPE3 storage/schema/magic, source requestu Yellowstone, five-lane
  readiness boundary, account filters, full-block reconciliation ani raw
  retention;
- vendored Pump IDL, semantics manifestu, ProgramData authority, RPC, GPA,
  snapshotu, backfillu lub imputacji;
- coverage/denominator/minimum gates, event CPI final-state bindings,
  output/window schemas, strategy path, V1/GO-D, Gatekeepera, OracleRuntime
  lub execution;
- zachowanego raw i historycznego diagnostic exact outputu.

Nie wykonuje provider I/O, preflightu ani recapture'u. Po reviewed clean
commicie dozwolona jest dokładnie jedna create-new offline requalification
immutable raw, z nowym executable digestiem i osobnym output directory.

## D4. Weryfikacja wymagana przed rekwalifikacją

```text
cargo fmt --all -- --check
cargo check --locked --offline -p seer --bin pump-exact-state-tape-v2
cargo test --locked --offline -p seer research_exact_tape_v2_semantics --lib --no-fail-fast
cargo test --locked --offline -p seer research_exact_tape_v2_materializer --lib --no-fail-fast
cargo test --locked --offline -p seer research_exact_tape_v2 --lib --no-fail-fast
cargo test --locked --offline -p ghost-core pump_research_exact_tape_v2 --lib --no-fail-fast
cargo test --locked --offline -p seer grpc_connection::tests --lib --no-fail-fast
cargo test --locked --offline -p seer --bin pump-exact-state-tape-v2 --no-fail-fast
cargo build --locked --offline --release -p seer --bin pump-exact-state-tape-v2
target/release/pump-exact-state-tape-v2 --help
git diff --check
git diff --cached --check
```

## D5. Wynik lokalnej walidacji

Przeszły lokalnie, locked/offline i bez provider I/O:

```text
cargo fmt --all -- --check                                                     PASS
cargo check --locked --offline -p seer --bin pump-exact-state-tape-v2          PASS
cargo test --locked --offline -p ghost-core pump_research_exact_tape_v2        5/5 PASS
cargo test --locked --offline -p seer research_exact_tape_v2                   87/87 PASS
cargo test --locked --offline -p seer research_exact_tape_v2_materializer      29/29 PASS
cargo test --locked --offline -p seer research_exact_tape_v2_semantics         11/11 PASS
cargo test --locked --offline -p seer grpc_connection::tests                   95/95 PASS
cargo test --locked --offline -p seer --bin pump-exact-state-tape-v2           1/1 PASS
cargo build --locked --offline --release -p seer --bin pump-exact-state-tape-v2 PASS
target/release/pump-exact-state-tape-v2 --help                                 PASS
git diff --check / git diff --cached --check                                   PASS
```

Wynik nie jest jeszcze claimem realnej `Qualified` capability. Przed jedyną
offline requalification wymagane są neutralny self-review, allowlist-only
commit i release build z dokładnego commita; raw oraz poprzedni diagnostic
output pozostają niezmienione.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```
