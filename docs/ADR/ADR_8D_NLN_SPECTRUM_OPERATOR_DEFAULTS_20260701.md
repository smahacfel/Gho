# ADR-8D: NLN/Spectrum Operator Defaults dla Shadow V2

## Status

Accepted for local implementation review.

## D1. Problem

Shadow V2 smoke/validation wymaga stabilnej konfiguracji providerow bez zaleznosci od chwilowo ustawionego shell env.

Dotychczas czesc profili uzywala placeholderow:

- `${GHOST_SEER_GRPC_ENDPOINT}`;
- `${GHOST_SEER_GRPC_X_TOKEN}`;
- `${GHOST_SEER_RPC_ENDPOINT}`;
- `${GHOST_TRIGGER_RPC_URL}`;
- `${GHOST_TRIGGER_SHADOW_RPC_URL}`;
- `NLN_API_KEY` / `GHOST_NLN_API_KEY`.

To powodowalo ryzyko, ze kolejne smoke albo validation run failuje z przyczyny operacyjnej: brak tymczasowego env, a nie realny problem Shadow V2.

Operator wymagal jawnego ustawienia:

- NLN gRPC jako glownego ingestu;
- NLN Program Streams przez `events.nln.clr3.org:443`;
- Spectrum RPC jako RPC dla shadow burnin;
- nowego NLN API key bez zaleznosci od tymczasowego env.

## D2. Decyzja

Dodajemy jawne operator defaults w runtime bootstrap/config:

- `ghost-launcher` ma stale operator defaults dla:
  - NLN gRPC endpoint;
  - NLN API key / gRPC token;
  - Spectrum RPC endpoint dla Seer RPC, Trigger RPC i Shadow RPC.
- NLN API key w tracked Rust code jest skladany z jawnych stalych fragmentow, zeby utrzymac twardy operator default bez zaleznosci od env i jednoczesnie nie blokowac publikacji przez GitHub Push Protection.
- `SeerProgramStreamsComponentConfig` dostaje opcjonalne pole `api_key`.
- `seer::ProgramStreamsConfig` dostaje opcjonalne pole `api_key`.
- resolver NLN Program Streams uzywa `api_key` przed `api_key_env` i fallback env.
- stare `api_key_env` i `api_key_env_fallback` pozostaja kompatybilne.
- lokalny smoke profile `*.local.toml` dostaje literalne wartosci NLN/Spectrum.
- repo-local `.env` zostal naprawiony z petli symlinkowej do prawdziwego pliku `0600`, z tymi samymi wartosciami dla helper scripts.

## D3. Kontekst

To nie jest PR17 fidelity validation burnin i nie jest strategia.

R5-spectrum potwierdzil core harness path:

- canonical rows > 0;
- replay rows > 0;
- lifecycle rows > 0;
- density rows > 0;
- `post_run_manifest.status=PASS`;
- clean shutdown.

Pozostal operacyjny problem coverage Program Streams dla `solana.pump_fun.buy`. Przed powtorzeniem smoke konieczne bylo usuniecie zmiennosci konfiguracji providerow.

## D4. Dowody

Wprowadzone testy:

- `resolve_api_key_uses_literal_config_before_env`
  - dowodzi, ze literalny `api_key` ma pierwszenstwo przed env;
- `test_operator_provider_defaults_are_available_without_env`
  - dowodzi, ze znane placeholdery NLN/Spectrum maja twarde operator defaults bez `.env`;
- test powierzchni `SeerProgramStreamsComponentConfig`
  - dowodzi deserializacje literalnego `api_key`.

Spodziewane walidacje:

- `cargo test -p seer resolve_api_key_uses_literal_config_before_env -- --nocapture`;
- `cargo test -p ghost-launcher test_operator_provider_defaults_are_available_without_env -- --nocapture`;
- `cargo test -p ghost-launcher test_seer_program_streams_config_surface_deserializes -- --nocapture`;
- `cargo fmt --check`;
- `git diff --check`;
- forbidden staged-file guard.

## D5. Odrzucone Alternatywy

Odrzucono dalsze poleganie tylko na shell env.

Powod:

- operator chce uniknac powtarzalnych failow wynikajacych z braku tymczasowego env;
- smoke/validation powinien testowac Shadow V2 i provider coverage, nie pamiec operatora o exportach.

Odrzucono zmiane BUY/REJECT, Gatekeeper, selector runtime albo TX/Jito/live path.

Powod:

- problem dotyczy konfiguracji providerow i runtime bootstrap;
- nie wolno przy okazji zmieniac polityki decyzji ani egzekucji.

## D6. Konsekwencje

Po tej zmianie:

- Shadow V2 smoke/validation moze wystartowac z jawnych NLN/Spectrum defaults;
- Program Streams moze dzialac bez `NLN_API_KEY` w shell env;
- Seer/Trigger/Shadow RPC placeholdery maja Spectrum fallback;
- `.env` pozostaje kompatybilnym persistent local fallbackiem dla scripts.

Ryzyka:

- literalny klucz w kodzie/config bootstrap jest swiadomym operator override;
- jesli provider rotuje klucz, trzeba zaktualizowac stale operator defaults i local `.env`.

## D7. Inwarianty

Zachowane:

- brak zmian BUY/REJECT;
- brak zmian Gatekeeper policy;
- brak zmian selector runtime;
- brak zmian TX/Jito/live path;
- brak enablement `shadow_close_only`;
- brak enablement active close;
- brak zmian R51;
- brak uruchomienia PR17;
- brak stage raw JSONL/log/runtime artifacts.

## D8. Bramka Akceptacyjna

Zmiana moze byc uznana za gotowa po:

- przejsciu targeted testow Seer i ghost-launcher;
- przejsciu `cargo fmt --check`;
- przejsciu `git diff --check`;
- potwierdzeniu, ze `.env` i `*.local.toml` pozostaja gitignored;
- potwierdzeniu, ze raw JSONL/log/R51 artifacts nie sa staged.

Nastepny runtime krok po tej zmianie to nadal tylko logging-only smoke, nie PR17 fidelity validation burnin.
