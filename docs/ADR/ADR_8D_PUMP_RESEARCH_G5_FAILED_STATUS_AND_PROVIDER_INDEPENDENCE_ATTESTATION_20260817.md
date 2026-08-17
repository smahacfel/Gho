# ADR-8D: Pump Research G.5 — failed-status authority i provider-independence attestation

**Data:** 2026-08-17

**Status:** IMPLEMENTED LOCALLY / SPECTRUM GO-E0.2 PASSED / COMBINED CERTIFY HOLD

**Task:** `PUMP_RESEARCH_G5_FAILED_STATUS_AND_PROVIDER_INDEPENDENCE_ATTESTATION`

## D0. Problem

GO-E0.1 potwierdził, że bounded provider-suitability porównuje transaction
identity, invocation class i failed status. Review wykazał jednak, że pełny
independent audit deklarował ten sam kontrakt, lecz jego decyzja wymagała tylko
zgodności identities i class counts. Przeciwna wartość failed status mogła więc
zostać oznaczona jako `Matched` i wejść do globalnego `Ready`.

Drugim blockerem była niezależność źródła audytowego. Różny provider ID,
hostname lub transport nie stanowi sam w sobie dowodu rozłączności failure
domains. Wymagany był create-new, hash-pinned artefakt operatora, związany z
konkretnym udanym suitability probe i bez możliwości podmiany go po provider
I/O, lecz przed publikacją exact outputu.

Historyczny GO-E0.1 korzystał z publicznego Solana RPC, nie ze Spectrum. Nie
wolno było retroaktywnie przypisać jego receiptu do innego providera.

## D1. Decyzja: jeden failed-status comparator

Provider-suitability i pełny audit korzystają od tej korekty z jednego
komparatora. Wynik zachowuje:

```text
identity_multiset_matches
invocation_class_counts_match
failed_status_multiset_matches
raw_failed_transaction_count
audit_failed_transaction_count
```

Pełny slot finding ma status `Matched` tylko przy trzech zgodnych multisetach.
Failed-status mismatch daje `SourceCoverageUnproven` i nie może prowadzić do
`Ready`. Globalny report zapisuje osobne raw i audit failed counts; dawne pole
`failed_transaction_count` pozostaje addytywnym, backward-compatible aliasem
raw count. Zmiana nie dotyka frozen `PumpResearchRawRecordV1`, nagłówka,
footera, framed storage ani golden fixtures.

## D2. Decyzja: atest jako obowiązkowe authority input combined audit

Combined `certify` przyjmuje wyłącznie kompletny zestaw:

```text
--qualification-audit-config
--provider-suitability-receipt
--provider-independence-attestation
--expected-provider-independence-sha256
```

Walidacja przed pierwszym provider I/O wymaga:

- regularnych, niesymlinkowych plików configu, receiptu i atestu;
- zgodnego expected SHA-256 atestu;
- `ready_for_full_audit` z kompletnymi identity/class/failed findings;
- tego samego run ID, G.4 epoch/range, configu, endpoint digestu, executable i
  GO-D raw controls;
- `attestation_status = verified_independent`;
- wszystkich jawnych assertion ustawionych na `true`;
- odrębnych provider IDs i entity names;
- dokładnego, jeszcze nieistniejącego planned exact outputu;
- braku literalnego endpointu, hostname'u i credentialu w atestacji.

Wszystkie authority inputs są hashowane ponownie bezpośrednio przed
utworzeniem exact outputu. Drift albo pojawienie się output path kończy się
fail-closed. Qualification report ustawia
`provider_identity_independence_verified = true` wyłącznie na podstawie
zweryfikowanego atestu i zapisuje jego SHA-256/BLAKE3/size.

## D3. Spectrum endpoint credential boundary

Protected audit config przechowuje wyłącznie root-only HTTPS origin i nazwę
dedykowanej zmiennej `GHOST_PUMP_RESEARCH_AUDIT_RPC_PATH`. Chroniona ścieżka
endpointu jest łączona z originem wyłącznie w pamięci. Nie może współistnieć z
header-token mode i nie korzysta z legacy RPC auth fallbacku.

Credential-byte scan GO-E0 i atestu obejmuje pełny resolved endpoint,
endpoint-path credential oraz ewentualny explicit header token. W Spectrum
GO-E0.2 exact-byte scan receiptu, logu, preparation snapshotu i atestu uzyskał
zero trafień pełnego endpointu, ścieżki credentialowej i hostname'u.

## D4. Wykonany Spectrum GO-E0.2

Po osobnym operator GO wykonano jedną bounded read-only operację:

```text
source run              = pump-research-1786909252793-3799414
audit provider          = spectrum-solana-mainnet-finalized-rpc
stream epoch            = 1
qualification range     = 439703838..=439708174
sample/attempted/matched= 17 / 17 / 17
unavailable             = 0
request attempts        = 17
provider elapsed        = 31 394 ms
status                  = ready_for_full_audit
exit status             = 0
raw write attempts      = 0
exact created           = false
certify/export/strategy = false / false / false
```

Artefakty:

```text
provider_suitability_receipt_v1.json
SHA-256 = 859ba278557e840a0f36440995561ea0c84ce438995c30b00b85a3c9e3154e5d
BLAKE3  = b54581debd31b8fdf9f203dc884879067e79cbcfbfadfc14fa09443e0e6061ed

/protected/operator/pump-research-audit-v1.toml
SHA-256 = 2266a154901a98c913887c941cde730e599f0a511b52be8020429b4435af2c4a
BLAKE3  = 46f7a670dc552a95e5740219c8a628a78d43b867b763c58bcd8f66e83348e5df

/protected/operator/provider_independence_attestation_v1.json
SHA-256 = 286a32fe87cd549ddc9f8e78ceccd99602ff54d271b67ab1979effb32ba6f9db
BLAKE3  = 3833e8b7ac00fadc84c2d2f19656344624ddc42acdcfa373aca3d9b8ae22ceeb
```

Atest wiąże dokładnie ten Spectrum probe, aktualny certifier executable,
config, endpoint digest, GO-D controls, G.4 range i planned exact output.
Oficjalne referencje potwierdzają produkty NoLimitNodes Yellowstone/Geyser i
Spectrum/Simply Staking RPC. Dokładne ASN oraz region labels pozostają jawnie
opisanymi deklaracjami operatora, nie wynikiem automatycznego network
discovery. Jest to świadomy podział odpowiedzialności: certifier dowodzi bytes
i bindings, operator bierze odpowiedzialność za prawdziwość physical-domain
assertions.

## D5. Testy i failure paths

Nowe regresje dowodzą:

1. wspólnego failed-status comparatora dla GO-E0 i pełnego audytu;
2. same identity i class counts, lecz przeciwny failed status, nigdy nie daje
   `Matched` ani `Ready`;
3. endpoint-path credential pozostaje poza config origin;
4. niezweryfikowany atest jest odrzucany przed provider I/O;
5. drift atestu jest odrzucany przed utworzeniem exact outputu;
6. częściowy zestaw combined-authority flags jest odrzucany przez CLI.

Końcowa lokalna weryfikacja:

```text
cargo check -p seer --lib                                  PASS
cargo check -p seer --bin pump-research-tape               PASS
research_tape_materializer                                 30 passed
research_tape filter                                       67 passed, 1 ignored
grpc_connection::tests                                     95 passed
rpc_http_client                                             6 passed
standalone CLI                                              7 passed
ghost-core frozen Pump Research V1                         10 passed
CS0 protobuf/descriptor                                     2 passed
parser parity                                               1 passed
future-capture supervisor                                  10 passed
cargo build --locked --release -p seer --bin pump-research-tape PASS
release capture-enabled harness                             1 passed
```

Fresh release harness uzyskał `8192 / 8192 / 8192`
received/admitted/accepted, zero dropów i gapów, jeden segment, clean writer,
`p99 = 191 ns` przy limicie `100 000 ns` oraz
`fatal -> source cancel = 54 518 910 ns` w testowym slow-I/O.

## D6. Wpływ, rollback i następna bramka

Zmiana jest research-only. Nie modyfikuje aktywnego Seera, requestu
Yellowstone, Event Busa, OracleRuntime, MaterializedFeatureSet, Gatekeepera,
execution ani strategii. Rollback oznacza pozostawienie combined certify w
stanie HOLD; nie wolno wrócić do logicznego `provider_id != primary` ani
ignorować failed-status mismatch.

Spectrum GO-E0.2 i atest są gotowymi authority inputs, lecz ten ADR nie daje
automatycznego GO do combined provider I/O. Do osobnego review obowiązuje:

```text
GO-D raw                       PASS / UNCHANGED
Spectrum GO-E0.2               READY_FOR_FULL_AUDIT
failed-status enforcement      PASS LOCALLY
provider-independence artifact HASH-PINNED / OPERATOR-ASSERTED
combined certify               HOLD / NOT RUN
exact Ready                    NOT CREATED
export / strategy / execution  NO-GO
```
