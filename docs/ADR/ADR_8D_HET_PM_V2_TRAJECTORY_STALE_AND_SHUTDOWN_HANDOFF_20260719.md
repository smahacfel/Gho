# ADR-8D: HET-PM V2 TrajectoryStale i shutdown-edge handoff noise

Status: `IMPLEMENTED / SHADOW ONLY`

Typ: ADR-8D / post-buy manager / runtime evidence

Data: `2026-07-19`

Repozytorium: `/root/Gho_dynamic_exit_v1_pr2b`

Uwaga o szablonie: wskazany w globalnych instrukcjach plik
`/root/Gho/docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym środowisku.
Dokument używa lokalnego układu D1--D8 stosowanego w repozytorium.

## D1. Problem

Run `shadow-het-pm-v2-authoritative-20260719-retry5` potwierdził, że aktywny
shadow manager zamyka pozycje, ale pozostawił dwa techniczne problemy:

1. `TrajectoryStale` stanowiło około jedną trzecią ticków HET. Przyczyną nie
   było fabrykowanie starych quote'ów, tylko brak świeżego stanu krzywej dla
   części pozycji po ucichnięciu poola. Manager poprawnie odmawiał sprzedaży
   na nieświeżych danych, ale Trailing/Vitality miały mniej realnych okazji do
   działania.
2. Końcówka kontrolowanego shutdownu generowała noise typu late handoff /
   closed queue. Część shadow BUY mogła skończyć się po rozpoczęciu shutdownu,
   kiedy PostBuyRuntime zdążył już zamknąć bezpośredni receiver.

## D2. Decyzja

Dodano dwie ograniczone zmiany techniczne.

### Shadow-only refresh stanu rynku

Profil HET-PM V2 może włączyć `post_buy_guardian.shadow_market_refresh`.
Mechanizm działa wyłącznie dla już otwartych pozycji shadow i wyłącznie wtedy,
gdy kanoniczny `AccountStateCore` nie ma świeżego stanu krzywej.

Refresh:

- używa read-only `getAccountInfo`;
- ma limit równoległości, cooldown per pozycja i timeout RPC;
- zapisuje wynik do `AccountStateCore` jako `UpdateSource::RpcRefresh`;
- nie buduje, nie wysyła i nie potwierdza żadnej transakcji;
- nie zmienia ścieżki live;
- nie retimestampuje starej ceny.

Jeżeli RPC nie odpowie, konto nie istnieje albo dekodowanie krzywej się nie
powiedzie, nowy snapshot nie powstaje i manager pozostaje fail-closed.

### Shutdown-edge handoff hardening

OracleRuntime dostaje jawny shadow-only sygnał `shutdown_requested`.
Po rozpoczęciu shutdownu nowy shadow BUY nie jest już przekazywany do
PostBuyRuntime. Zostaje zapisany typed skip jako `shadow_skipped_shutdown`.

PostBuyRuntime nie zamyka natomiast bezpośredniego receivera natychmiast po
soft drainie. Po soft deadline utrzymuje receiver otwarty, dopóki producent
się nie zamknie albo nie minie hard deadline. To ogranicza końcówkowy hałas
`queue closed` bez blokowania procesu bez końca.

## D3. Granice bezpieczeństwa

Zmiana nie wprowadza live execution.

Zmiana nie przesuwa ownership sprzedaży poza istniejącą shadow-only ścieżkę
HET-PM V2.

CrashGuard nadal używa surowej kanonicznej trajektorii. Świeża obserwacja
runtime może poprawić trajectory używaną przez HET, ale nie luzuje ani nie
zastępuje raw evidence CrashGuarda.

Refresh nie jest nowym źródłem decyzji BUY/REJECT. Działa po otwarciu pozycji
shadow i służy wyłącznie utrzymaniu świeżego stanu krzywej dla managera
sprzedaży.

## D4. Konfiguracja

Aktywny profil dodaje:

```toml
[post_buy_guardian.shadow_market_refresh]
enabled = true
stale_after_ms = 1500
interval_ms = 250
per_position_cooldown_ms = 1000
max_requests_per_cycle = 8
rpc_timeout_ms = 750
```

Launcher wymaga jawnego `trigger.shadow_run.shadow_rpc_url`, gdy refresh jest
włączony. Brak endpointu jest błędem konfiguracji przed startem.

Rollback tej części to:

```toml
[post_buy_guardian.shadow_market_refresh]
enabled = false
```

## D5. Implementacja

Zmienione obszary:

- `ghost-brain/src/guardian/post_buy/config.rs` — konfiguracja i walidacja
  `ShadowMarketRefreshConfig`;
- `ghost-brain/src/guardian/post_buy/engine.rs` — lista stale targetów i
  materializacja trajectory z najświeższą obserwacją runtime;
- `ghost-core/src/account_state_core/types.rs` — nowe `UpdateSource::RpcRefresh`;
- `ghost-launcher/src/components/post_buy_runtime.rs` — bounded background
  refresh task, konfiguracja RPC i shutdown direct-drain hard cap;
- `ghost-launcher/src/oracle_runtime.rs` — shadow-only shutdown guard przed
  późnym handoffem;
- `ghost-launcher/src/main.rs` — preflight wymaga RPC dla refreshu.

## D6. Testy

Wykonane lokalnie:

```text
cargo test -q -p ghost-launcher post_buy_runtime_shutdown_keeps_direct_receiver_open_until_producer_closes --lib
cargo test -q -p ghost-launcher shadow_market_refresh_requires_read_only_rpc_endpoint --lib
cargo test -q -p ghost-brain het_trajectory_uses_fresh_runtime_observation_without_refreshing_crash_guard --lib
cargo test -q -p ghost-brain stale_shadow_market_refresh_targets_use_raw_canonical_state_age --lib
cargo check -p ghost-brain -p ghost-core -p ghost-launcher --lib
```

Wynik: wszystkie powyższe komendy zakończyły się sukcesem. Repozytorium nadal
emituje istniejące ostrzeżenia niezwiązane z tą zmianą.

## D7. Weryfikacja runem

Następny run:

```text
shadow-het-pm-v2-authoritative-20260719-retry6
```

Cel porównania względem `retry5`:

- spadek udziału `TrajectoryStale`;
- brak wzrostu `QuoteUnavailable`;
- brak końcowego noise `Confirmed BUY could not be handed off` /
  `Failed to send shadow-backed PostBuySubmitted event` / `queue closed`;
- dalsza obecność terminalnych wyjść shadow i zwolnienia pozycji;
- brak paniki runtime.

Jeżeli `TrajectoryStale` pozostanie wysokie, przyczyną do sprawdzenia w
następnej kolejności będzie nie brak odświeżania, tylko konkretne błędy RPC,
dekodowania bonding curve albo pozycje, które migrują poza wspierany route.

## D8. Rollback

Rollback refreshu:

```toml
[post_buy_guardian.shadow_market_refresh]
enabled = false
```

Rollback shutdown-edge guardów wymaga przywrócenia poprzedniej logiki drainu
PostBuyRuntime i usunięcia shadow-only skipu `shutdown_requested`. Nie wpływa
to na live execution, ponieważ zmiana jest ograniczona do shadow handoff path.
