# ADR-8D: Prospective Pump Exact-State Tape V2 — jawne Base64 dla ProgramData RPC

**Data:** 2026-08-24

**Status:** IMPLEMENTED LOCALLY / LOCAL OFFLINE VALIDATION PASS / INDEPENDENT REVIEW PASS / NO RETRY / NO NEW PREFLIGHT / NO RAW V3 CREATED

**Typ:** ADR-8D / standalone prospective V2 ProgramData authority / fail-closed RPC representation

> Globalny szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie jest dostępny w tym
> środowisku. Dokument zachowuje lokalny układ ADR-8D używany przez istniejące
> ADR-y V2.

## D0. Potwierdzony problem

Po prawidłowym replacement sealed preflightu pierwszy jawnie autoryzowany
capture nie utworzył raw V3. Zatrzymał się przed alokacją katalogu raw przy
drugim, wymaganym odczycie `getAccountInfo`: finalized Pump ProgramData.

`observe_program_data_receipt_v2()` ustawił tylko commitment `finalized` w
`RpcAccountInfoConfig`, pozostawiając kodowanie danych konta na domyślnym
Base58. Pump Program jest wystarczająco mały, lecz rzeczywisty ProgramData
przekracza limit 128 bajtów Base58. Spectrum RPC poprawnie odrzucił żądanie
JSON-RPC `-32600` z wymaganiem Base64.

Nie jest to awaria Spectrum ani NLN gRPC: Yellowstone nie został otwarty, nie
powstał raw start/completion receipt, a old PRXTAPE2 evidence pozostało
nietknięte.

## D1. Decyzja

Oba i tylko oba dozwolone odczyty ProgramData authority — Pump Program oraz
ProgramData wskazane przez Program — wysyłają teraz literalnie:

```rust
RpcAccountInfoConfig {
    encoding: Some(UiAccountEncoding::Base64),
    commitment: Some(CommitmentConfig::finalized()),
    ..RpcAccountInfoConfig::default()
}
```

Base64 zmienia wyłącznie JSON-RPC transport representation. Nie zmienia
odczytywanych pubkeyów, commitmentu, owner/layout validation, bytes używanych
do BLAKE3, ProgramData semantics authority ani liczby odczytów.

## D2. Regresja

Istniejący lokalny mock ProgramData RPC wymaga teraz dla obu żądań:

```text
method   = getAccountInfo
encoding = base64
```

Nadal wymaga dokładnie dwóch kont: canonical Pump Program i ProgramData
wyprowadzonego z jego upgradeable-loader state. Test nadal zakazuje
`getProgramAccounts`, `getMultipleAccounts`, snapshotu, RPC backfillu oraz
jakiegokolwiek provider I/O.

## D3. Zakres wyłączony

Korekta nie zmienia:

- PRXTAPE3 storage/schema/magic, recordera, five-lane readiness boundary,
  source requestu Yellowstone, dwóch account filters ani full-block lane;
- configu operatora, endpointów, credentiali, provider roles, Pump IDL lub
  semantics manifestu;
- exact-state qualifiera, coverage/denominator/minimum gates, JSONL/window
  schemas ani strategy path;
- V1/GO-D, GO-E, Gatekeepera, OracleRuntime lub execution.

Nie wykonuje retry capture'u. Korekta wymaga nowego independent review,
commita i replacement sealed preflightu przed odrębną decyzją operatorską o
następnym bounded capture.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```

## D4. Plan weryfikacji

Po korekcie wymagane są co najmniej:

```text
cargo fmt --all -- --check
cargo test --locked --offline -p seer v3_program_data_receipt_rpc_uses_base64_and_reads_only_program_and_programdata_accounts --lib --no-fail-fast
cargo test --locked --offline -p seer research_exact_tape_v2 --lib --no-fail-fast
cargo check --locked --offline -p seer --bin pump-exact-state-tape-v2
cargo build --locked --offline --release -p seer --bin pump-exact-state-tape-v2
git diff --check
git diff --cached --check
```

Wszystkie te kontrole pozostają lokalne/offline. Nie uruchamiają sealed
preflightu, capture'u, RPC, Yellowstone ani strategii.

## D5. Wynik lokalnej walidacji

Przeszły lokalnie i offline:

```text
cargo fmt --all -- --check
cargo check --locked --offline -p seer --bin pump-exact-state-tape-v2
cargo test --locked --offline -p seer v3_program_data_receipt_rpc_uses_base64_and_reads_only_program_and_programdata_accounts --lib --no-fail-fast
cargo test --locked --offline -p seer research_exact_tape_v2 --lib --no-fail-fast                 # 72 passed
cargo test --locked --offline -p seer research_exact_tape_v2_materializer --lib --no-fail-fast    # 23 passed
cargo test --locked --offline -p seer research_exact_tape_v2_semantics --lib --no-fail-fast       # 9 passed
cargo test --locked --offline -p seer grpc_connection::tests --lib --no-fail-fast                 # 95 passed
cargo test --locked --offline -p ghost-core pump_research_exact_tape_v2 --lib --no-fail-fast      # 5 passed
cargo test --locked --offline -p seer --bin pump-exact-state-tape-v2 --no-fail-fast               # 1 passed
cargo build --locked --offline --release -p seer --bin pump-exact-state-tape-v2
target/release/pump-exact-state-tape-v2 --help
git diff --check
git diff --cached --check
```

Nowo zbudowana, nie-sealed binarka deweloperska ma SHA-256:

```text
efe45b3b4cefd0a9ee096972d79f6d6e3058cc1efdf6b4123993ded5ac58bacb
```

Hash nie jest nową authority operatorską. Replacement preflight może powstać
wyłącznie po independent review PASS, osobnym allowlist-only commicie i
odrębnej zgodzie operatorskiej.

## D6. Fresh independent review

Świeży, read-only review aktualnego dirty diffu potwierdził `PASS` przy
`P0=0`, `P1=0`, `P2=0`. Niezależnie potwierdził:

- jeden wspólny `RpcAccountInfoConfig` z `Base64 + finalized` dla obu
  `getAccountInfo`, używany na starcie i completion;
- literalne `params[1].encoding == "base64"` dla obu żądań lokalnego mocka,
  prawidłowe dekodowanie odpowiedzi Base64 oraz nadal dokładnie dwa dozwolone
  konta;
- brak driftu w PRXTAPE3, Yellowstone requestach, configu/operator roles,
  semantics, qualifierze, Gatekeeperze i runtime;
- minimalność manifestu/lockfile oraz zgodność ADR z kodem;
- brak błędów whitespace dla tracked i nowego untracked ADR.

Review nie wykonywał edycji, stagingu, commita, preflightu, capture'u,
provider/RPC I/O ani dostępu do credentiali.
