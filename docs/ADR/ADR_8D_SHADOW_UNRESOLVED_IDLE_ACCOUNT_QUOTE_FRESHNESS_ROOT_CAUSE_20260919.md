# ADR-8D: Shadow unresolved — brak potwierdzania aktualności wyceny cichego konta

**Data:** 2026-09-19
**Status:** ROOT CAUSE VERIFIED / REPAIR PROPOSED / RUNTIME UNCHANGED
**Zakres:** diagnoza shadow runa `predator-v11-20260919-thresholds-r2`.

## D0. Cel i granica zadania

Użytkownik zażądał ustalenia rzeczywistej przyczyny `position_unresolved`,
zaczynając od najtańszej wiarygodnej weryfikacji kanałów zewnętrznych,
oraz propozycji minimalnej naprawy. Nie wdrażano poprawki ani nie restartowano
procesu. PID 443042 działał w tmux `ghost-predator-v11-thresholds-20260919-r2`.

Wskazany szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` ani jego odpowiedniki pod
`/root/Gho` i `/root/Gho_ingest` nie istnieją w tym środowisku. Zachowano układ
D0–D8 zgodny z lokalnymi raportami ADR-8D.

## D1. Dowód z kanałów zewnętrznych

Endpoint gRPC: `grpc.nln.clr3.org:443`. Endpoint HTTP RPC:
`spectrum-02.simplystaking.xyz`. Seer RPC, Trigger RPC i shadow RPC wskazują
ten sam endpoint; testowano go bez kopiowania credentiali do raportu.

- 25 porównań processed RPC z bieżącym gRPC BlockMeta: `RPC - gRPC` od -1 do 0
  slotów, mediana 0. RTT RPC: minimum 61.221 ms, mediana 70.632 ms,
  maksimum 385.892 ms.
- Pierwsza subskrypcja w 37.784 s: 863 AccountUpdate, 143 BlockMeta,
  427 komunikatów slot. Jedyny `CANCELLED` wynika z celowego zamknięcia
  kanału przez skrypt po zakończeniu próby.
- Osobna próba obejmująca transakcje i konta: 1474 transakcje,
  580 AccountUpdate, 79 BlockMeta, 0 błędów.
- Porównanie hashy bajtów kont przy uwzględnieniu slotu RPC: 37/37 zgodnych.
  Dwie dodatkowe próbki nie miały historii strumienia poprzedzającej context
  slot odczytu i nie są dowodem ani zgodności, ani rozbieżności.
- Publiczny RPC Solana w trzech dodatkowych odczytach również zwracał
  aktualną okolicę slotów. Jego RTT 769–784 ms oznacza, że nie wolno
  traktować końców tych odczytów jako równoczesnych z szybszym RPC.
- Jeden processed `getMultipleAccounts` zwrócił 49/49 badanych curve accounts
  w 71.517 ms: poprawny owner Pump, discriminator i `complete=false`.

Nie ma dowodu, że wielosekundowy wiek snapshotów tych przypadków pochodził
z opóźnionego providera. Bieżące pomiary nie certyfikują całego ośmiogodzinnego
runa ani maksymalnego opóźnienia 50 ms.

## D2. Weryfikacja historycznych przypadków

Zamrożono listę 49 unresolved. Pobrane historie zawierały łącznie 40312
wpisów; wszystkie 49 obejmowały ostatni slot zapisany w lifecycle.

Badany przedział zaczyna się po ostatnim sample slocie i kończy konserwatywnie
na `blockTime <= terminal_ms/1000 - 2`. Margines chroni przed utożsamieniem
sekundowego blockTime z lokalnym timestampem. Historia sygnatur nie jest
rekonstrukcją `write_version` ani bajtów kont w dowolnym historycznym momencie.

- 44 konta: zero udanych transakcji w tym przedziale.
- 5 kont: 21 udanych transakcji odwołujących się do kont; sprawdzono ich
  wykonanie. Zero wywołań programu Pump, zero zmian lamportów curve,
  zero zmian token balances. Samo wystąpienie writable account w TX nie jest
  dowodem zmiany jego rezerw.
- W obserwacjach HET zawierających faktyczną decyzję V1: 39 proposal
  `quote_required:inactivity` oraz 10 `quote_required:absolute_max_hold`.
- Wiek danych w chwili inactivity proposal: 30038–30972 ms.
  Dla absolute max hold: 5065–27917 ms. Wszystkie przekraczają 1500 ms.
- Wiek danych w unresolved: 10065–36357 ms, mediana 35494 ms.

## D3. Potwierdzona przyczyna po naszej stronie

Polityka V1 czeka 30 s braku aktywności lub 120 s wieku pozycji. Wycena
wyjścia odrzuca jednak snapshot po 1500 ms. Canonical snapshot ma czas
ostatniej przyjętej obserwacji danego konta. W ciszy rynkowej ten czas
naturalnie się starzeje, nawet gdy provider dostarcza aktualny strumień.

Wyjście czasowe nie ma czynnej ścieżki potwierdzenia aktualnego stanu takiego
konta. `resolve_shadow_exit_truth_for_policy` ponownie ocenia zachowany
snapshot; nie wykonuje żądania RPC. Po 5 s recovery następuje terminalny
`BLOCKED_BY_DATA` bez quote'u i bez PnL.

Konkretny przypadek `2A8WbcRYNC6YWcdVx7nBptdBFjEupQ6kRKMd8kpBhvqu`:

1. Ostatnia próbka: 09:51:18.607 UTC, slot 448372197.
2. Inactivity proposal: 09:51:49.579 UTC, wiek próbki 30972 ms.
3. Unresolved: 09:51:54.579 UTC, wiek próbki 35972 ms.

To jest niekompletna integracja wyjść czasowych ze źródłem świeżej wyceny.
Odmowa sfabrykowania fill/PnL na starej cenie pozostaje poprawna.

## D4. Dlaczego istniejący refresh nie rozwiązuje problemu

`ShadowMarketRefreshConfig::default()` ustawia `enabled=false`; sekcji
`shadow_market_refresh` nie ma w snapshocie konfiguracji r2. Scheduler jest
więc wyłączony. Samo ustawienie `enabled=true` nie wystarczy:
`AccountStateReducer::apply_rpc_refresh` jedynie porównuje hash payloadu,
zwracając matches/diverges, i celowo nie mutuje canonical state ani freshness.
Nie przekazuje również PM odrębnego potwierdzenia aktualności quote'u.

Miejsca w kodzie:

- `ghost-brain/src/guardian/post_buy/exit_policy_v1.rs`: `evaluate_prequote`.
- `ghost-brain/src/guardian/post_buy/engine.rs`: `shadow_exit_stale_after_ms`,
  `resolve_shadow_exit_truth_for_policy`, `handle_shadow_quote_failure`.
- `off-chain/components/trigger/src/entry_price_extractor.rs`:
  `resolve_shadow_exit_sample_with_source`.
- `ghost-launcher/src/components/post_buy_runtime.rs`:
  `spawn_shadow_market_refresh`, `refresh_shadow_market_target`.
- `ghost-core/src/account_state_core/reducer.rs`: `apply_rpc_refresh`.

## D5. Proponowana minimalna naprawa

Przy stale quote dla istniejącego proposal uruchomić ograniczony,
asynchroniczny odczyt konkretnego curve accountu w dotychczasowym 5 s budget.
Odczyt ma potwierdzać aktualność tego samego canonical payloadu, a nie
przepisywać `AccountStateCore`.

Po sprawdzeniu pubkey, owner, discriminator, kontekstu processed RPC,
minimalnego context slotu oraz zgodności hashy utworzyć niezmienne
potwierdzenie odczytu powiązane z position id/epoch/revision, ilością tokenów,
canonical version/hash i czasami request/response. Ograniczyć czas ważności
potwierdzenia; uwzględnić czas żądania, a nie tylko moment jego zakończenia.

Quote może korzystać z canonical reserves wraz z takim aktualnym
potwierdzeniem identycznych danych. Rozbieżny hash, zbyt stary context,
zmiana pozycji lub brak odpowiedzi nadal blokują i wymagają ponownej
oceny. Potwierdzenie nie może resetować inactivity, dodawać transakcji,
zmieniać trajektorii ani odświeżać canonical timestamps. W JSONL zapisać
osobno wiek mutacji i źródło/czas potwierdzenia quote'u.

Nie wystarczy zwiększyć TTL do 35 s, podmienić timestampu na `now()` ani
użyć postępu slotów innego poola. Propozycja nie zmienia Gatekeepera,
progów TP/SL, max hold, live execution ani historycznych unresolved.

## D6. Weryfikacja i kryteria poprawki

Uruchomiono dwa istniejące testy z uprzednio zbudowanej binarki
`target/debug/deps/ghost_brain-76d84b96f8401f5c` z 2026-09-16:

- `shadow_runtime_time_stop_rejects_stale_snapshot_without_emitting_fill`: 1/1 PASS.
- `unchanged_rpc_refresh_is_observation_only_without_vitality_activity`: 1/1 PASS.

Nie budowano nowej binarki runtime. Testy potwierdzają obecne zachowanie,
nie skuteczność jeszcze niewdrożonej poprawki.

Wymagane przyszłe kryteria: brak TX przez 30 s + aktualne RPC z identycznymi
bajtami -> jedno zamknięcie z quote evidence; identity/hash/context mismatch,
RPC timeout oraz spóźniona odpowiedź dla zmienionej pozycji -> brak fill;
inactivity/trajectory/canonical state nie zmieniają się wskutek odczytu.

## D7. Dowody i stan procesu

Pełny zestaw pomiarów oraz skrypty odczytu znajdują się w
`/tmp/shadow-root-cause-20260919/`. Plik `summary.json` wskazuje liczby,
ograniczenia i nazwy prób. Skrypty czytają credentiale z dotychczasowego
pliku środowiska i nie zapisują ich do wyników. Artefakty w `/tmp` mają
charakter roboczy i nie są repozytoryjnym archiwum.

Nie modyfikowano kodu/configów runtime, nie restartowano procesu,
nie poprawiano wstecznie PnL. Zmieniono tylko checkpoint i niniejszy raport.

## D8. Samokontrola zakresu

Wniosek opiera się na bezpośrednim porównaniu providerów, payloadów kont,
historiach przypadków, faktycznych proposal V1 oraz aktywnym kodzie.
Nie utożsamiono blockTime z transport latency, RTT z opóźnieniem publikacji,
samego writable account z mutacją ani przeszłego sample age z globalnym lagiem.
Propozycja naprawy zachowuje fail-closed i osobne authority canonical state.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
GO-D nie był przedmiotem tych odczytów.
