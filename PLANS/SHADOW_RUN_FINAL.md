# SHADOW_RUN_FINAL

## Cel

Doprowadzić `shadow run` do stanu, w którym:

- buduje i symuluje poprawny BUY bez zgadywania układu kont,
- nie psuje source of truth,
- nie psuje osi czasu eventów,
- nie miesza warstw odpowiedzialności,
- nie dodaje niestabilnego RPC-reconstruct na hot path,
- nie rozjeżdża produkcyjnej logiki Gatekeepera / OracleRuntime / SnapshotEngine.

## Wybrana ścieżka

### Decyzja: `strict canonicalization`, nie pełny `replay-first`

Powód:

- pełny `replay-first` wymagałby kopiowania całego observed account layout z realnego chain tx jako głównego źródła BUY accounts,
- to jest szybkie jako hack, ale grozi zatruciem pipeline przez błędne / ewoluujące observed pola,
- już widzieliśmy, że observed `fee_recipient` potrafi być błędny, więc ślepy replay nie jest bezpieczny jako systemowy model.

### Model docelowy

Shadow BUY ma być budowany z:

- kanonicznych stałych / allowlist dla kont globalnych,
- kanonicznego `token_program` z właściciela minta,
- kanonicznego `creator_pubkey` z `DetectedPool` / verified fallback,
- kanonicznego `associated_bonding_curve` wyłącznie z bezpiecznego źródła,
- `buy_variant` tylko z parsera source instruction albo z jawnego defaultu po walidacji,
- observed tx mają służyć jako **pomocniczy sygnał**, nie jako ślepe źródło prawdy.

W praktyce:

- `strict canonicalization` dla kont globalnych,
- `verified observed override` tylko dla tych pól, które są stabilne i walidowalne,
- zero “pierwsze niepuste pole wygrywa” bez walidacji.

## Co jest realnym problemem

To nie jest problem “jak zbudować transakcję Solany”.

Problemem jest to, że obecny shadow path przez długi czas składał BUY z mieszanki:

- `DetectedPool`,
- `PoolTransaction`,
- `FirstTxFallback`,
- lokalnego PDA derivation,
- observed override z parsera,
- RPC fetchy,
- osobnego RPC do symulacji.

To dało:

- `metadata_missing`,
- `AccountNotFound`,
- `InsufficientFundsForRent`,
- `InstructionError(0, ...)`,
- `InstructionError(1, Custom(1))`.

Czyli nie jeden bug, tylko kaskadę błędów warstwowych.

## Stan końcowy, do którego dążymy

Po wdrożeniu końcowym ma być tak:

1. `seer` emituje tylko sprawdzone dane observed.
2. `OracleRuntime` kanonizuje BUY metadata i BUY accounts do jednego obiektu.
3. `TriggerComponent` niczego nie zgaduje.
4. `DirectBuyBuilder` dostaje gotowy, zweryfikowany zestaw kont.
5. `shadow run` używa jednego spójnego view RPC dla prepare + simulate.
6. `shadow telemetry` zawsze pokazuje prawdziwą przyczynę failure, nie alias.

## Plan wykonawczy

### Etap 1. Zamknąć model danych BUY accounts

#### Krok 1.1
Wprowadzić jeden jawny typ kanoniczny, np. `CanonicalShadowBuyAccounts`.

Plik:

- `ghost-launcher/src/oracle_runtime.rs`
- opcjonalnie nowy plik helpera, np. `ghost-launcher/src/shadow_buy_accounts.rs`

Zakres pól:

- `global_config`
- `fee_recipient`
- `token_program`
- `creator_pubkey`
- `buy_variant`
- `associated_bonding_curve`

Cel:

- koniec z luźnym `BuyAccountOverrides` składanym z przypadkowych źródeł,
- OracleRuntime ma budować jeden obiekt kanoniczny,
- Trigger ma już tylko mapować ten obiekt do buildera.

#### Krok 1.2
Rozdzielić pola na 3 klasy:

- `canonical constants`
- `verified dynamic`
- `unsafe observed`

Reguły:

- `global_config`: tylko allowlist / canonical
- `fee_recipient`: tylko allowlist / canonical
- `token_program`: tylko z właściciela minta
- `creator_pubkey`: z `DetectedPool.creator`, a jeśli fallback, to tylko gdy przejdzie walidację
- `buy_variant`: z parsera source ix albo bezpieczny default po walidacji
- `associated_bonding_curve`: tylko jeśli przejdzie walidację względem minta / bonding curve

### Etap 2. Uciąć observed poison u źródła

#### Krok 2.1
W `seer` oznaczyć observed pola BUY jako:

- `verified`
- albo `unverified`

Pliki:

- `off-chain/components/seer/src/types.rs`
- `off-chain/components/seer/src/binary_parser.rs`
- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/events.rs`

Cel:

- nie przenosić dalej “pierwszego lepszego” `fee_recipient`,
- observed value ma mieć status jakości, nie tylko `Option<String>`.

#### Krok 2.2
Wyeliminować observed `fee_recipient` jako źródło prawdy.

Pliki:

- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- `off-chain/components/trigger/src/direct_buy_builder.rs`

Reguła:

- `fee_recipient` nie może już pochodzić z tx telemetry,
- builder bierze tylko canonical / allowlisted fee recipient.

To jest najważniejszy fix pod obecne `InstructionError(1, Custom(1))`.

### Etap 3. Skanonizować `buy_variant`

#### Krok 3.1
Zdefiniować jedną tabelę mapowania variantów BUY.

Pliki:

- `off-chain/components/seer/src/binary_parser.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `off-chain/components/trigger/src/direct_buy_builder.rs`

Reguła:

- `DISC_BUY` -> `LegacyBuy`
- `DISC_PUMP_BUY_ROUTED` -> `RoutedExactSolIn`
- `DISC_SWAP_BUY_EXACT_QUOTE_IN`:
  decyzja jawna, jedna w całym systemie, bez dwóch różnych interpretacji

Cel:

- zero domyślnego “unknown” w runtime, jeśli parser już widział variant,
- zero milczącego spadania do routed bez kontroli.

#### Krok 3.2
Jeśli `buy_variant` jest nieznany, BUY ma być:

- albo jawnie odrzucony,
- albo zbudowany tylko po walidacji against observed account layout.

Nie może być cichego fallbacku “może zadziała”.

### Etap 4. Skanonizować `associated_bonding_curve`

#### Krok 4.1
Przestać ufać observed `associated_bonding_curve` bez walidacji.

Pliki:

- `ghost-launcher/src/oracle_runtime.rs`
- `off-chain/components/trigger/src/direct_buy_builder.rs`

Reguła:

- jeśli override jest observed, trzeba sprawdzić, czy odpowiada kanonicznemu ATA dla:
  - właściwego bonding curve PDA,
  - właściwego minta,
  - właściwego token programu

Jeśli nie pasuje:

- override out,
- builder używa kanonicznej derivation.

#### Krok 4.2
Dodać helper walidacyjny:

- `validate_associated_bonding_curve(mint, bonding_curve, token_program, candidate) -> bool`

Cel:

- odróżnić “realny override” od śmiecia z parsera / rozjechanego chain tx.

### Etap 5. Zakończyć chaos metadata path

#### Krok 5.1
Utrzymać `buffered_tx_fallback`, ale zrobić z niego oficjalny, kontrolowany tor.

Plik:

- `ghost-launcher/src/oracle_runtime.rs`

Reguły:

- fallback buduje syntetyczny `DetectedPool` tylko z danych minimalnie potrzebnych do shadow,
- wszystkie pola fallback muszą być jawnie oznaczone źródłem:
  - `runtime_registry`
  - `runtime_state_snapshot`
  - `buffered_tx_fallback`
  - `late_new_pool`

Cel:

- `metadata_missing` ma oznaczać realny brak danych, a nie to, że dane były, tylko nie miały oficjalnego kształtu.

#### Krok 5.2
Wyciąć martwe / legacy ścieżki, które próbują robić BUY poza OracleRuntime BUY path.

Plik:

- `ghost-launcher/src/components/trigger/component.rs`

Cel:

- jeden autorytatywny tor BUY,
- brak konkurencyjnych lokalnych override buildów.

### Etap 6. Jedno źródło RPC dla `ShadowOnly`

#### Krok 6.1
Zostawić obecną zasadę:

- `ShadowOnly`: prepare i simulate używają tego samego `shadow_rpc_url`

Pliki:

- `ghost-launcher/src/components/trigger/component.rs`
- `ghost-launcher/src/components/trigger/shadow_run.rs`

Cel:

- koniec rozjazdu payer balance / rent / mint visibility między prepare a simulate.

#### Krok 6.2
Dodać jawny log source-of-truth:

- `prepare_rpc_url_kind=primary|shadow`
- `simulate_rpc_url_kind=shadow`

Cel:

- łatwa diagnoza, czy znowu nie ma dwóch różnych view świata.

### Etap 7. Skanonizować klasyfikację błędów

#### Krok 7.1
Rozdzielić finalne klasy failure:

- `shadow_metadata_missing`
- `shadow_account_not_visible`
- `shadow_insufficient_balance`
- `shadow_ata_prepend_error`
- `shadow_buy_account_mismatch`
- `shadow_rpc_transport_error`

Pliki:

- `ghost-launcher/src/components/trigger/shadow_run.rs`
- `ghost-launcher/src/oracle_runtime.rs`

Cel:

- koniec z `shadow_unknown_error` dla znanych klas,
- telemetry ma od razu mówić, czy padł prepend, payer, czy buy accounts.

#### Krok 7.2
`InstructionError(0, ...)` i `InstructionError(1, ...)` muszą być rozbijane jawnie.

Reguła:

- `Instruction 0` = prepend / payer / ATA
- `Instruction 1` = buy ix

To ma być utrwalone w JSONL i logach.

### Etap 8. Twarde testy kontraktowe

#### Krok 8.1
Dodać testy `OracleRuntime`:

- bad observed `fee_recipient` jest ignorowany,
- bad observed `associated_bonding_curve` jest ignorowany,
- unknown `buy_variant` nie przechodzi cicho,
- `buffered_tx_fallback` daje `shadow_ready=true`, jeśli ma komplet minimalnych danych

Plik:

- `ghost-launcher/src/oracle_runtime.rs`

#### Krok 8.2
Dodać testy `TriggerComponent`:

- builder dostaje tylko canonical fee recipient,
- shadow-only prepare używa shadow RPC,
- `create_user_ata=true` + mały payer daje precheck failure przed simulate,
- `create_user_ata=false` nie dolicza ATA rent

Plik:

- `ghost-launcher/src/components/trigger/component.rs`

#### Krok 8.3
Dodać testy `DirectBuyBuilder`:

- kolejność kont dla `LegacyBuy`
- kolejność kont dla `RoutedExactSolIn`
- canonical fee recipient wchodzi na konto 1
- `associated_bonding_curve` override tylko po walidacji

Plik:

- `off-chain/components/trigger/src/direct_buy_builder.rs`

#### Krok 8.4
Dodać testy `seer`:

- dedup nie gubi jakości observed account fields,
- parser mapuje variant deterministycznie,
- observed `fee_recipient` może być oznaczony jako `unverified`

Plik:

- `off-chain/components/seer/src/binary_parser.rs`
- `off-chain/components/seer/src/lib.rs`

### Etap 9. Migracja wdrożeniowa

#### Krok 9.1
Wdrożyć etapami:

1. telemetry + walidacja + logi
2. canonical fee recipient
3. canonical associated bonding curve
4. variant hardening
5. usunięcie legacy path

Powód:

- żeby nie rozjebać produkcji jednym dużym strzałem,
- i żeby każda zmiana była mierzalna w logach.

#### Krok 9.2
Metryki sukcesu po wdrożeniu:

- spadek `shadow_unknown_error` do zera lub blisko zera
- spadek `InstructionError(1, Custom(1))`
- zniknięcie `fee_recipient=CebN5...q1VV...` z logów prepared request
- `metadata_missing` tylko dla realnie martwych przypadków
- `InstructionError(0, ...)` oddzielone od buy ix failures

## Kolejność wykonania w praktyce

1. Zrobić `CanonicalShadowBuyAccounts`
2. Wywalić observed `fee_recipient` jako źródło prawdy
3. Zweryfikować `associated_bonding_curve`
4. Domknąć mapowanie `buy_variant`
5. Utrzymać `buffered_tx_fallback` jako oficjalny metadata path
6. Wyciąć legacy BUY path poza OracleRuntime
7. Dopić telemetry error taxonomy
8. Odpalić runtime verification na świeżych poolach

## Minimalne acceptance criteria

Temat uznajemy za zamknięty dopiero wtedy, gdy:

- shadow BUY dla nowych mintów przechodzi bez `metadata_missing`,
- `shadow_only` nie ma prepare/simulate RPC split,
- builder nie przyjmuje zatrutego `fee_recipient`,
- `InstructionError(1, Custom(1))` przestaje dominować,
- a jeśli jeszcze występuje, telemetry wskazuje jedno konkretne konto / klasę błędu.

## Wniosek końcowy

Nie należy dalej łatać objawów pojedynczymi ifami.

Należy:

- ustawić **jeden model kanoniczny BUY accounts**,
- odciąć observed poison,
- zostawić `TriggerComponent` jako głupi builder,
- a `OracleRuntime` zrobić jedynym miejscem składania prawdy dla shadow BUY.

To jest najkrótsza droga do działającego `shadow run` bez psucia source of truth i reszty systemu.
