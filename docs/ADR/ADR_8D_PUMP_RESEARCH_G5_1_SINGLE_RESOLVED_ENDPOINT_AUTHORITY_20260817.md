# ADR-8D: Pump Research G.5.1 — single resolved endpoint authority

**Data:** 2026-08-17

**Status:** IMPLEMENTED LOCALLY / NO PROVIDER I/O / COMBINED CERTIFY HOLD

**Task:** `PUMP_RESEARCH_G5_1_SINGLE_RESOLVED_ENDPOINT_AUTHORITY`

## D0. Problem

G.5 poprawnie wiązało digest resolved Spectrum endpointu z suitability
receiptem i provider-independence attestation. Walidator zwracał jednak tylko
audit config i validated attestation. Full audit ponownie wywoływał
`config.resolve_connection()`, czyli drugi raz odczytywał mutowalne globalne
environment.

Formalnie możliwy był przebieg:

```text
resolve A -> receipt/attestation validation -> env drift -> resolve B -> audit
```

Nie ma dowodu, że taki drift wystąpił w GO-E0.2; receipt Spectrum pozostaje
ważnym i niezmienionym bounded proof. Szczelina blokowała wyłącznie przyszły
combined audit, ponieważ report mógł zostać związany atestem A mimo odpytywania
endpointu B.

Review wskazał również, że combined validator sprawdzał ogólne liczniki i
booleany suitability receiptu, lecz nie odtwarzał dokładnego deterministic
sample planu ani unikalności slotów/roles.

## D1. Decyzja: connection resolved dokładnie raz

Walidator zwraca teraz jeden lokalny authority object:

```text
PumpResearchValidatedCombinedAuditAuthorityV1 {
    audit_config,
    resolved_connection,
    audit_rpc_endpoint_blake3,
    provider_independence,
}
```

`resolved_connection` powstaje podczas walidacji configu, suitability receiptu
i atestu. Ten sam obiekt jest przekazywany do
`run_independent_source_completeness_audit_v1()` i dalej do konstruktora
standalone RPC clienta. Full audit nie ma już wywołania
`config.resolve_connection()`.

Qualification report zapisuje `audit_rpc_endpoint_blake3` z validated
authority, nie z nowego odczytu środowiska. Przed utworzeniem exact outputu
revalidation wymaga, aby digest zapisany w wyniku auditu był identyczny z
digestem zwalidowanego atestu.

Resolved connection nie implementuje `Debug`, nie jest serializowane i
pozostaje w pamięci tylko przez czas combined operation. Endpoint path/header
credential nadal nie trafia do reportu, logu ani exact outputu.

## D2. Decyzja: suitability receipt musi odpowiadać odtworzonemu planowi

Combined validator ponownie wykonuje na immutable raw:

```text
qualification_range_selection_v1
-> build_provider_suitability_plan_v1
-> collect_provider_suitability_raw_transactions_v1
```

Następnie wymaga dokładnej zgodności:

- G.4 `stream_epoch`, start i end;
- first/mid/last oraz bounded-burst slots;
- representative roles direct top-level, inner CPI, router CPI, v0 loaded
  address i failed transaction;
- unikalnego zbioru finding slots;
- uporządkowanej listy roles dla każdego slotu;
- raw identity count, failed count i invocation-class counts;
- audit count/class equality deklarowanej przez matched finding;
- pustych raw-only/audit-only identities;
- config concurrency/retry/timeout i frozen GO-E0 limits;
- sumy per-finding request attempts.

Dowolny duplicate, role drift albo niespójny licznik blokuje combined audit
przed provider I/O.

## D3. Regresje publicznej granicy

Test `validated_combined_endpoint_survives_process_environment_drift`:

1. tworzy kryptograficznie poprawny frozen raw run;
2. buduje exact deterministic suitability receipt i atest dla endpointu A;
3. waliduje authority;
4. zmienia dedykowany endpoint-path env na B;
5. potwierdza, że klient oraz report zachowują A i nie adoptują B.

Test `public_combined_invalid_authority_performs_zero_provider_requests`:

1. uruchamia publiczne
   `certify_pump_research_raw_run_with_qualification_audit_v1()`;
2. podaje atest ze statusem innym niż `verified_independent`;
3. używa lokalnego, nonblocking loopback listenera jako request counter;
4. wymaga typed authority error, zero accepted connections i braku exact
   outputu.

Ten sam fixture odrzuca także duplicate finding slot i selection-role drift.
Testy nie wykonują zewnętrznego RPC.

## D4. Nowy executable i nowy atest

Kod G.5.1 zmienia combined certifier executable:

```text
target/release/pump-research-tape
SHA-256 = 97251d5427e89a762a22ca5c06c29c7e7e9ab43c235bd6f5e82fb31b4c3617cf
BLAKE3  = 42fb85a5a2f4a2301be2ecbdc3c7494114ed357920b16f3fe49ea60a851c3739
bytes   = 12 546 816
mode    = 0700
```

Atest G.5 o SHA-256 `286a32fe...` pozostał bez zmian. Utworzono osobny,
create-new artefakt:

```text
/protected/operator/provider_independence_attestation_g5_1_v1.json
SHA-256 = 25b55aba36a90ce1fca2ec5528cd2438234e2ce584161f6021b61e2372ecb204
BLAKE3  = 8f702ebd2556d987b507cca1db4d048256e5bd640404d76fdedcbbeadd0dd92d
bytes   = 4 266
mode    = 0600
```

Nowy atest zachowuje:

- Spectrum GO-E0.2 receipt i jego executable;
- protected config i resolved endpoint digest;
- GO-D binding/start/completion;
- G.4 epoch/range;
- planned exact output;
- dotychczasowe operator assertions;

oraz wiąże nowy G.5.1 combined certifier executable. Nie wykonano nowego
provider probe, ponieważ GO-E0.2 receipt i jego schema pozostają niezmienione.

## D5. Wpływ i rollback

Zmiana jest research-only. Nie dotyka frozen raw V1, capture ingress, requestu
Yellowstone, active Seer runtime, Event Busa, OracleRuntime,
MaterializedFeatureSet, Gatekeepera, execution ani strategii.

Rollback oznacza pozostawienie combined audit w stanie HOLD. Nie wolno wrócić
do drugiego `resolve_connection()` ani akceptować suitability receiptu bez
rekonstrukcji sample authority.

Po G.5.1 nadal obowiązuje:

```text
GO-D raw                        PASS / UNCHANGED
Spectrum GO-E0.2                READY_FOR_FULL_AUDIT / UNCHANGED
single resolved connection      PASS LOCALLY
deterministic suitability plan  PASS LOCALLY
new G.5.1 attestation           CREATED / HASH-PINNED
combined certify                HOLD / NOT RUN
exact Ready                     NOT CREATED
export / strategy / execution   NO-GO
```

## D6. Weryfikacja lokalna

Bez provider I/O, `certify`, utworzenia exact outputu ani zmiany GO-D
przeszły:

```text
cargo fmt --all -- --check                                      PASS
cargo check -p seer --lib                                       PASS
cargo check -p seer --bin pump-research-tape                    PASS
cargo test -p seer research_tape_materializer --lib             31/31 PASS
cargo test -p seer research_tape --lib                          68 PASS, 1 ignored
cargo test -p seer rpc_http_client --lib                         6/6 PASS
cargo test -p seer grpc_connection::tests --lib                 95/95 PASS
cargo test -p seer --bin pump-research-tape                     7/7 PASS
cargo test -p ghost-core pump_research_tape --lib               10/10 PASS
cargo test -p seer --test pump_research_tape_cs0                 2/2 PASS
pr1d_v1_v2_parser_digests_remain_frozen                          1/1 PASS
python3 -m unittest scripts/test_pump_research_capture_supervisor.py
                                                                  10/10 PASS
```

Release capture-enabled harness wykonał rzeczywisty bounded ingress/writer
flow:

```text
received / admitted / accepted = 8192 / 8192 / 8192
dropped / gaps                 = 0 / 0
persisted missing events       = 0
segments                       = 1
writer clean                   = true
writer error                   = null
capture abort                  = false
receive hand-off p99           = 201 ns
SLA                            = 100 000 ns
fatal -> source cancel         = 53 111 523 ns
```

Test publicznej granicy invalid authority potwierdził zero requestów do
loopback providera i brak exact outputu. Test driftu endpointu potwierdził, że
zmiana process environment po walidacji nie zmienia endpointu używanego przez
full audit.
