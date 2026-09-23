# ADR-8D: Pump Research Evidence Tape V1.1 — PR-A hermetyczny build i standalone RPC auth isolation

**Data:** 2026-08-16

**Status:** IMPLEMENTED / LOCAL-ONLY VERIFICATION PASSED / NO NEW PROVIDER I/O

**Task:** `PUMP_RESEARCH_TAPE_PR_A_BUILD_AUTH_ISOLATION_CORRECTION`

## D0. Problem

Pierwsza wersja sealed preflight poprawnie wiązała source snapshot, fresh
release binary i operator receipt, ale `Command::new("cargo")` dziedziczył
pełne environment procesu operatora. Named gRPC/RPC credential mógł zatem być
widoczny dla Cargo, rustc, compiler wrappera albo build-scriptu i potencjalnie
trafić do trwałego `release/build.log`.

Ponadto ProgramData start/completion receipt dla `rpc_auth_token = None`
używał legacy-aware generic RPC clienta. Na hostach objętych legacy fallbackiem
mogło to pozwolić na niejawne użycie Yellowstone credentialu jako RPC auth.

Są to problemy preflight/capture provenance i research-only RPC isolation.
Nie dotyczą frozen raw V1 codec ani aktywnego runtime Ghosta.

## D1. Decyzja: hermetyczne child environment świeżego builda

Fresh preflight build odtąd:

1. odrzuca przed utworzeniem sealed receipt każdy niepusty parent override,
   który zmienia compiler, wrapper, Rust flags lub release profile;
2. wywołuje Cargo przez `env_clear()` z minimalnym allowlistem:
   controlled `PATH`, fresh `HOME`, fresh `CARGO_HOME`, fresh
   `CARGO_TARGET_DIR`, `CARGO_NET_OFFLINE=true` i deterministic terminal
   color. Uruchamia bezpośrednio zahashowane Cargo i rustc wybranego
   toolchainu, nie rustup proxy;
3. usuwa named gRPC/RPC auth environment variables z child procesu;
4. buduje w fresh Cargo home, który może tylko odczytać offline cache/index/git
   DB. Parent Cargo config, credentials i checkouty nie są przekazywane;
5. zapisuje canonical paths i oba digests faktycznie użytych Cargo oraz rustc;
6. skanuje wszystkie regular files finalnego bundle'a dokładnymi bytes
   skonfigurowanych credentiali przed utworzeniem finalnego receipt.

Skan nie wypisuje wartości credentialu. Trafienie przerywa preflight i nie
publikuje `operator_preflight_receipt_v1.json`; pozostawiony katalog jest
incomplete forensic evidence, a nie sealed bundle.

## D2. Decyzja: jawne ProgramData RPC auth modes

PR-A ProgramData receipt używa wyłącznie:

```text
Some(configured RPC token) -> explicit standalone auth client
None                       -> explicit standalone no-auth client
```

No-auth constructor nie odczytuje legacy process-global headerów. W
szczególności nie może odziedziczyć `GHOST_SEER_GRPC_X_TOKEN` tylko dlatego,
że endpoint RPC należy do hosta z legacy policy. Konstruktor z defaultowym
timeoutem jest publiczną granicą PR-A; materializer może używać niższego
timeout-configurable helpera, nadal bez legacy auth.

## D3. Regresje i dowód lokalny

Dodano regresje dla:

- odrzucenia każdego compiler/wrapper/flag/profile override przed buildem;
- dokładnego minimalnego child environment;
- wykrycia synthetic credential w sealed bundle przed publikacją receipt;
- wyboru `StandaloneNoAuth` na publicznej ścieżce ProgramData receipt;
- braku `x-api-key`, `x-token`, `Authorization` i `Proxy-Authorization` w
  standalone no-auth header surface.

Dotychczasowe testy raw V1 i parser parity pozostają niezależne. Korekta nie
zmienia `PumpResearchRawRecordV1`, nagłówków/footerów, manifestu raw runu,
`SeerConfig`, active `connect_geyser()`, Gatekeepera, MFS, execution ani
legacy clientów używanych poza research-only path.

## D4. Status operacyjny i rollback

Do zakończenia full local-only review tej korekty zabronione są kolejne realne
preflighty, capture, provider qualification i export. Nie naprawiamy
retroaktywnie starych receiptów ani datasetów; nowa semantyka receipt jest
celowo różna i stare receipt failują closed.

Rollback to niewykonywanie preflight/capture. Nie wolno przywracać
odziedziczonego child environment, legacy-aware no-auth fallbacku, optional
receipt ani logowania credentialu w celu diagnozy.
