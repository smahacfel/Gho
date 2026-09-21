# ADR-8D: Kanoniczny creator i odświeżenie quote BUY przed dispatch

## D0. Dyspozycja

Usunąć potwierdzone przyczyny błędów wejścia z runa r16 bez zmiany progów Gatekeepera: błędne `creator_vault` kończące się `ConstraintSeeds(2006)`, quote zbudowane ze starszego stanu kończące się `TooMuchSolRequired(6002)` oraz mylące zaliczanie zatrzymań przed dispatch jako nieudanych symulacji.

## D1. Stan wejściowy

Końcowy materiał r16 zawierał 41 zakończonych wywołań `simulateTransaction`: 25 poprawnych oraz 16 odrzuconych przez Pump. Piętnaście odrzuceń miało kod `2006`, jedno kod `6002`. Osobne cztery przypadki miały `dispatch_attempted=false`, `simulation_attempted=false` i nie dotarły do RPC simulation.

Parser konta Pump odczytywał rezerwy i `complete`, ale nie przenosił 32-bajtowego `creator` znajdującego się po fladze `complete`. Aktywna ścieżka legacy mogła następnie nadać authority wartości `DetectedPool.creator`, mimo że nie była ona execution truth dla `creator_vault`.

Quote legacy był budowany wcześniej niż ostateczny dispatch. Runtime nie wykonywał ostatniego porównania z najnowszym stanem `AccountStateCore` i nie zapisywał kompletnego boundary pozwalającego porównać stan quote z bankowym slotem symulacji.

## D2. Przyczyny

1. `2006`: utrata canonical creator na granicy decode i późniejsze naruszenie authority. Builder poprawnie wyprowadzał PDA, ale z niewłaściwego creatora.
2. `6002`: quote nie był ponownie związany z najnowszym canonical snapshotem bezpośrednio przed dispatch. R16 nie zapisywał wystarczających danych do rozdzielenia zmiany curve, wieku strumienia i różnicy lokalnego modelu.
3. Cztery zatrzymania przed dispatch miały powód kończący się `simulation_load_not_ready`, chociaż `simulateTransaction` nie zostało wywołane. Latch canonical state był wyłączony w profilu.

## D3. Kontrakt

- `creator` dla `legacy_buy` pochodzi z konta bonding curve należącego do programu Pump.
- `DetectedPool.creator` pozostaje metadanym i nie uzyskuje execution authority dla legacy BUY.
- Obserwowany `creator_vault` musi być równy PDA wyprowadzonemu z canonical creator. Rozbieżność kończy ścieżkę przed dispatch z `creator_vault_canonical_mismatch`.
- Ostatnia wycena przed dispatch korzysta wyłącznie z `AccountStateCore`, czyli z canonical strumienia gRPC. RPC nie staje się authority quote.
- Stan starszy niż istniejąca polityka `live_preflight_max_state_age_slots` kończy się `stale_canonical_curve`.
- Zmiana canonical snapshotu w trakcie przebudowy transakcji kończy się fail closed.
- Latch pozostaje `shadow_only`, ma budżet 25 ms, `fail_closed=true` i `allow_rpc=false`.
- Zatrzymanie przed dispatch zapisuje `dispatch_attempted=false` i `simulation_attempted=false`.

## D4. Zmiana

Rozszerzono lenient Borsh decode konta Pump o pole `creator` z bieżącego 81-bajtowego payloadu po discriminatorze. Wartość jest akceptowana jako kanoniczna tylko dla ownera programu Pump i przechodzi addytywnie przez Seer IPC, launcher event oraz `AccountStateUpdate` do `CanonicalPoolState`. Aktualizacja kompatybilna bez creatora zachowuje wcześniejszą wartość canonical zamiast ją usuwać. Nowe pola struktur serializowanych zostały dopisane na końcu, aby nie przesuwać istniejącego prefiksu pozycyjnego.

Usunięto aktywną promocję `DetectedPool.creator` do authority legacy. Finalny kontekst P37 synchronizuje źródło i flagę authority z overrides po materializacji canonical state. Builder udostępnia jedną funkcję wyprowadzania creator-vault PDA; ta sama funkcja służy budowie i walidacji.

Bezpośrednio po finalnym prechecku, a przed shadow/live dispatch, legacy request jest przebudowywany z najnowszego `CanonicalPoolState`. Zachowane są blockhash, opłata priorytetowa, tip, join metadata, diagnostyka latcha i istniejące telemetry preparation. Po przebudowie runtime ponownie odczytuje canonical state i odrzuca request, jeżeli slot, write version, hash lub rezerwy zmieniły się w trakcie operacji.

`ShadowV2EntryBoundaryPayload` zapisuje addytywnie `quoted_tokens_out`, `quote_state_age_slots` oraz `quote_refresh_status`. W połączeniu z istniejącymi polami boundary utrwala slot i timestamp state, `latest_observed_slot`, hash, rezerwy, kwotę SOL, minimum tokenów i dane źródłowe. Istniejący raport symulacji zapisuje slot banku RPC oraz dla `6002` wartości `Left`/`Right`, wymagany koszt i shortfall. Parser błędu `2006` zapisuje nazwę konta Anchor i oba adresy.

Powód finalnego braku gotowości manifestu nazwano `legacy_buy_pre_dispatch_manifest_not_ready`. Profile shadow burn-in włączają bounded latch 25 ms bez RPC. Wyniki odświeżenia, braku creatora, mismatchu vault i starego state mają odrębne klasy outcome.

Nie zastąpiono twardego modelu 100 bps inną zgadywaną stałą. Repozytorium nie ma obecnie canonical, strumieniowego `fee_config` podłączonego do tej ścieżki quote; istniejący decoder opłat w `rug_scalp_v2` pobiera authority przez RPC i nie spełnia kontraktu tej naprawy. Boundary jawnie zachowuje ograniczenie `FEE_MODEL_ASSUMPTION_BONDING_CURVE_DEFAULT_100BPS`. Świeży re-quote ogranicza historyczny drift, ale nie daje obietnicy zera `6002` przy zmianie stanu po ostatnim evencie ani bitowej niezgodności modelu opłat.

## D5. Testy

- Decode canonical creator z bajtów konta Pump oraz brak authority przy obcym ownerze.
- Przeniesienie i zachowanie canonical creator w `AccountStateCore`.
- Zastąpienie wykrytego creatora canonical wartością oraz jawny mismatch observed vault.
- Brak promocji `DetectedPool.creator` dla legacy BUY.
- Synchronizacja P37 authority po canonical materialization.
- Odrzucenie mismatchu przed zbudowaniem transakcji.
- Ponowne przeliczenie quote na nowszym canonical state oraz fail closed po przekroczeniu polityki wieku.
- Latch: świeży odczyt, timeout oraz niepełna identity; wszystkie jako pre-dispatch bez fałszywej flagi symulacji.
- Parsery `ConstraintSeeds` z kontem/Left/Right oraz `TooMuchSolRequired` z wyliczeniem shortfall.

## D6. Wynik weryfikacji

Wszystkie uruchomione testy celowane zakończyły się PASS:

- Seer canonical Pump creator: 1/1.
- `AccountStateCore` store/preserve creator: 1/1.
- Trigger builder wyprowadza creator-vault PDA z canonical creator: 1/1.
- Launcher canonical creator i vault: 5/5 w filtrze `canonical_creator`.
- P37 final context po materializacji: 1/1.
- Re-quote, boundary i polityka stale: 1/1.
- State readiness latch: 3/3.
- Parser konta Anchor Left/Right: 1/1.
- Parser shortfall `6002`: 1/1.
- Walidacja profilu latch shadow-only: 1/1.

`cargo test` dla celowanych modułów skompilował pełne testowe targety bibliotek `seer`, `ghost-core` i `ghost-launcher`. `git diff --check` zakończył się bez błędów.

Preflight pełnego profilu poprawnie załadował TOML, Ghost Brain 1000/7/5/4, shadow-only oraz nową sekcję latch. Cały preflight nie przeszedł w tej sesji z przyczyn środowiskowych: brak skonfigurowanej ścieżki keypair w bieżącym środowisku, niedostępna sieć RPC/gRPC w sandboxie i brak uprawnienia do bindu portu 9090. Nie jest to dowód gotowości do uruchomienia runa.

## D7. Przegląd

Niezależny przegląd finalnej zmiany wykrył i poprawił dwa problemy przed zakończeniem:

1. nowe pola creator były początkowo wstawione w środku struktur serializowanych; przeniesiono je na koniec;
2. kontekst P37 zachowywał stare źródło authority po poprawnej canonical materialization; teraz odczytuje finalne overrides.

Nie dodano RPC do authority quote, nie zmieniono progów Gatekeepera, nie zmieniono PnL ani zasad wyjścia, nie włączono live execution. Dirty checkout zawiera wiele wcześniejszych zmian; ten ADR nie przypisuje ich do niniejszej poprawki.

## D8. Wdrożenie

Nie uruchomiono nowego runa i nie zbudowano ani nie podmieniono binarki produkcyjnej. W chwili końcowej kontroli nie działał proces `ghost-launcher`. Zmiana wymaga normalnego build/preflight w środowisku z właściwymi sekretami, siecią i wolnym portem przed następnym burn-in.

Szablon `/root/Gho/docs/ADR/ADR_8D_SZABLON.md` nie istnieje w bieżącym filesystemie; zachowano układ D0-D8 używany przez istniejące ADR repozytorium.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE

GO-D nie jest wejściem tej zmiany runtime.
