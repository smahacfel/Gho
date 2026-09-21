# ADR-8D: Konfigurowalny minimalny licznik SELL w Gatekeeper V2

## D0. Dyspozycja

Dodać do Gatekeeper V2 próg minimalnej liczby transakcji SELL analogiczny do istniejącego `min_buy_count`, ponieważ bieżący kontrakt Phase 1 nie pozwalał wymagać ani utrwalać progu SELL.

## D1. Stan wejściowy

`GatekeeperBuffer` już klasyfikował każdą przyjętą transakcję jako BUY albo SELL i utrzymywał prywatny `sell_count`. Konfiguracja Phase 1 zawierała jednak wyłącznie `min_tx_count`, `min_unique_signers` oraz `min_buy_count`. Log `GatekeeperBuyLog` zapisywał `buy_count` i `min_buy_count`, ale nie zapisywał analogicznych pól SELL.

## D2. Przyczyna

Brak dotyczył kontraktu konfiguracyjnego i audytowego, nie ingestu. SELL-e były liczone, lecz nie można było użyć ich liczby jako warunku Phase 1 ani odtworzyć z rekordu decyzji, jaki próg SELL obowiązywał.

## D3. Kontrakt

- `min_sell_count` jest częścią `GatekeeperV2Config`.
- Wartość domyślna wynosi `0`, więc stare profile zachowują dotychczasowe decyzje.
- Wartość dodatnia jest czwartym warunkiem ilościowym Phase 1 obok minimalnej liczby TX, sygnatariuszy i BUY.
- Warunek obowiązuje w trybie standard, long, checkpointach DOW oraz w terminalnej ewaluacji opartej na `MaterializedFeatureSet`.
- Trwały `gatekeeper_v2_config_payload` zapisuje skonfigurowane `min_sell_count`.
- Rzeczywista liczba SELL pozostaje dokładnie odtwarzalna z zamrożonego rekordu v33 jako `total_tx_evaluated - buy_count`.

## D4. Zmiana

Dodano pole `min_sell_count: usize` z kompatybilnym domyślnym zerem. Wszystkie aktywne kontrole Phase 1 uwzględniają licznik SELL. Komunikaty timeout/core-fail i diagnostyka konfiguracji pokazują wartości `sells=actual/required`.

Kanoniczny profil `ghost-brain/ghost_brain_config.toml` deklaruje jawnie `min_sell_count = 0`, dzięki czemu nowe ustawienie jest widoczne i audytowalne bez zmiany zachowania istniejącego runu. Wartość dodatnia wymaga osobnej decyzji kalibracyjnej.

Nie zmieniono zamrożonego schematu `GatekeeperBuyLog` v33. Istniejący `gatekeeper_v2_config_payload` serializuje pełną konfigurację i dlatego zapisuje nowy próg bez naruszenia top-level v33. Rzeczywisty licznik SELL jest wyprowadzany jako `total_tx_evaluated - buy_count`, zgodnie z niezmienionym kontraktem Gatekeepera, w którym każda przyjęta transakcja jest klasyfikowana binarnie jako BUY albo SELL. Reason chain i logi terminalne pokazują `sells=actual/required`.

## D5. Testy

- Brak pola w starym TOML daje `min_sell_count = 0`.
- Jawne `min_sell_count` przechodzi deserializację.
- Phase 1 z dodatnim progiem SELL nie przechodzi dla strumienia samych BUY i przechodzi po wymaganym SELL.
- `gatekeeper_v2_config_payload` zapisuje próg SELL, a reason/log diagnostics pokazują licznik rzeczywisty i wymagany.

## D6. Wynik weryfikacji

Wyniki testów i kompilacji są rejestrowane w opisie PR po zakończeniu implementacji. `cargo fmt --all -- --check` oraz `git diff --check` pozostają obowiązkowe.

## D7. Przegląd

Zmiana nie modyfikuje sposobu klasyfikacji transakcji, SSOT cech, progów BUY ani kolejności polityk. Domyślne zero zachowuje decyzje istniejących profili. Dodatni próg SELL jest jawnie opt-in i może zmienić BUY na TIMEOUT wyłącznie w profilu, który go ustawi. Zamrożony top-level `GatekeeperBuyLog` v33 pozostaje bez zmian.

## D8. Wdrożenie

Nowy próg nie zostaje automatycznie aktywowany w istniejących profilach. Operator może dodać `min_sell_count = N` w sekcji `[gatekeeper_v2]`; brak pola pozostawia wartość `0`.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE

GO-D nie jest wejściem tej zmiany runtime.
