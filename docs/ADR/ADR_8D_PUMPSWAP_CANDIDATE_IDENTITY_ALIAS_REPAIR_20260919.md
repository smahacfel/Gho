# ADR-8D: Fałszywe konflikty tożsamości kandydatów i błędne przypisywanie pooli PumpSwap

## D0. Zakres

Użytkownik 2026-09-19 zlecił ustalenie rzeczywistej przyczyny osobnego problemu `candidate identity aliases disagree` oraz minimalną działającą naprawę. Zakres obejmuje tożsamość zdarzeń Seera i rejestr CandidateIntegrity, testy oraz wdrożenie do shadow. Bez zmiany progów, strategii, PnL, canonical timestamps ani wcześniejszego mechanizmu potwierdzania świeżości wyjścia.

Szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` oraz odpowiedniki pod `/root/Gho` i `/root/Gho_ingest` są nieobecne. Zachowano lokalny układ D0–D8.

## D1. Objawy i dowody

R4: w wycinku logu od 16:13:24 do 16:18:17 UTC wystąpiło 17 869 wierszy `candidate identity aliases disagree`. Dowody i kopie źródeł sprzed poprawki znajdują się w `/tmp/ghost-candidate-identity-20260919/`.

Punktowy odczyt `getMultipleAccounts` z kontekstem 448456008 potwierdził, że pool `7S54GPtjdYW2wug5gxWRkiEd5sVXVbtGVfRcsFYA1hUd` należy do PumpSwap i ma ten sam mint co obserwowana krzywa Pump. Sam wspólny mint nie jest konfliktem tożsamości poola. Oficjalny kontrakt PumpSwap dopuszcza wiele pooli tej samej pary: <https://github.com/pump-fun/pump-public-docs/blob/main/docs/PUMP_SWAP_README.md>.

Replay publicznej transakcji `DcBDHZ58yqFCWh58REJhUr3FLXqjRihuKxyH2G6BiaWUw9Z45jeTazQ3TEJ5pp7y395nQQZsPbT8UwWyMskwxua`, slot 448405529, sukces on-chain: instrukcja wskazuje pool `Gf7sXMoP8iRw4iiXmJ1nq4vxcRycbGXy5RL8a8LnTd3v` i mint USDC. Parser przed poprawką emituje dla niego dwa trade: USDC z ordinal19 i `FuriVrfyZgnXjABipp7ZdGsMCoRCgVXYp4LbdPNB1HrC` z ordinal24. To lokalna sprzeczność wytworzona z jednej transakcji, odtworzona bez sieci.

## D2. Przyczyny

1. `CandidateIntegrityRegistry` i jego terminalne tombstones utrzymywały indeks `by_mint` jako globalnie jednoznaczną tożsamość poola. Sygnał innego poola tego samego tokena inkrementował generację pierwszego kandydata, zmieniał jego outcome na `PrimaryRawCoverageIncomplete` i unieważniał jego guard. To mogło anulować prawidłowy BUY przed submit.
2. Instrukcja PumpSwap zawiera pool i mint, ale dekoder nie przekazywał tej relacji do istniejącego rejestru przed deduplikacją z CPI. CPI bez pola mint używało salda signera z całej wielopoolowej transakcji lub wcześniej błędnego cache. Stąd dwa różne minty dla jednego poola w podanym replay.
3. `hydrate_trade_mapping` remapował poprawny adres poola PumpSwap na wcześniejszy alias obserwacji tego mintu, pozostawiając niezmienioną surową obserwację. Launcher wymaga zgodności pary pool/mint z surowym dowodem, więc takie zdarzenie traciło pokrycie canonical.

Kontrola wdrożenia r5 ujawniła resztkową ścieżkę błędu parsera: transakcja `3bQJ8KPUtA98e6otyDiFrnXSV8pWyhTRYFLt3KJuMd66RWzi1J6PEwmbqNzXvo1Wf8h1HV3uHdUVRNRzg4yiw7TK`, slot448462658, dotyczy poola `6VcDLAMaKGEQLX7YiR8JnwUnoyU9mMJuMSYZnK9XmVTN` z parą token/PUMP, bez SOL. Instrukcja była poprawnie odrzucana przez `normalize_swap_pair`, ale CPI omijało tę kontrolę, przyjmując mint PUMP i jego ilość jako SOL. Nowy replay odtworzył to przed domknięciem poprawki (`non-sol-red.log`).

Nie przypisujemy automatycznie każdego historycznego `CandidateIntegrity` cancellation temu mechanizmowi. Ogólne `PrimaryRawCoverageIncomplete` może mieć inne udokumentowane przyczyny; nowy test odtwarza konkretną ścieżkę unieważnienia guardu przez drugi pool.

## D3. Minimalna poprawka

- Usunięto `by_mint` z aktywnego rejestru i tombstones. Pozostaje para `(pool_amm_id, mint)` oraz indeks `by_pool`: inny mint tego samego poola nadal blokuje kandydata.
- W istniejących gałęziach dekodowania PumpSwap buy/sell relacja pool→mint z kont konkretnej instrukcji trafia do pomocniczego rejestru przed deduplikacją i rozpoznaniem CPI.
- Remapowanie do aliasu obserwacji jest pomijane, gdy trade ma surowy dowód. Zgodny adres źródłowy dociera do IPC bez zmiany dowodu.
- CPI buy/sell stosuje tę samą kontrolę pary co bezpośrednia instrukcja. Minty są odczytywane z właściwej instrukcji swap tego poola, z pominięciem innych discriminatorów o odmiennym układzie kont. Para bez SOL nie może wrócić przez CPI jako fikcyjna transakcja SOL. Używany jest istniejący typed reason `pool_or_mints_invalid`.

## D4. Kontrakty

Oddzielne poole tego samego mintu nie dzielą generacji, dowodów Ready, pending apply receipts ani terminalnej historii. Same-pool/different-mint pozostaje fail-closed, również po retirement. Nie przyznano parserowemu cache authority nad AccountStateCore ani Gatekeeperem. Nie zmieniono schematów JSONL, progów ani shadow/live boundary.

## D5. Weryfikacja

- RED na kodzie przed zmianą: 2 testy izolacji pooli o wspólnym mincie, replay realnej transakcji (2 trade zamiast1), test IPC zmieniającego adres surowego poola.
- GREEN: 35/35 testów `candidate_integrity`; replay działa dla pustego i błędnego cache; test zgodności IPC/raw przechodzi. Istniejący test ciągłości aliasu dla zdarzeń bez raw również przechodzi.
- Pełny Seer po poprawce: 590 PASS, 12 FAIL, 3 ignored. Pełny Seer na dokładnej kopii sprzed zmiany: 586 PASS, 14 FAIL, 3 ignored. Wszystkie 12 błędów po zmianie występuje także na baseline; brak nowych niepowodzeń. Nie naprawiano niezwiązanych testów PumpPortal/synthetic/WAL.
- Dowody: `parser-red.log`, `registry-red.log`, `seer-red.log`, `registry-green.log`, `seer-green.log`, `seer-baseline.log`, `test-comparison.json` w katalogu zadania.
- Po domknięciu non-SOL bypass: pełny Seer590PASS/13FAIL/3ignored, wszystkie13FAIL obecne również na bazie sprzed zadania (baseline14FAIL). Zestaw parsera130PASS/1wcześniejszyFAIL. Oba replaye transakcji oraz IPC/raw PASS. Dowody `non-sol-red.log`, `parser-final-green.log`, `seer-final-green.log`; źródła końcowe `source-sha256-r6.json`, diff `implementation-final.diff`.

## D6. Wdrożenie

Release build PASS (9m15s). Pierwszy preflight blokował wyłącznie port9090 zajęty przez r4. Po łagodnym SIGINT przy zerowej liczbie aktywnych pozycji r4 zakończył się, a pełny preflight r5 przeszedł bez zmiany konfiguracji portu.

Pośredni run `predator-v11-20260919-candidate-identity-r5` działał od 16:28:10 UTC. Jego kontrola ujawniła opisany wyżej non-SOL bypass CPI. Został zatrzymany łagodnie o 16:48:24 przy 9 zamkniętych i 0 aktywnych pozycjach.

Końcowy run `predator-v11-20260919-candidate-identity-r6` działa od **16:49:32 UTC**, PID **565326**, tmux `ghost-predator-v11-candidate-identity-20260919-r6`. Release build i preflight PASS. Binarka SHA256 `d314e9ef6c91c0b2b6af1316affaff15a0d2b2d26d9c84ed4a7123c5ce21cef8`, zweryfikowana przez `/proc/565326/exe`. Profil różni się od r4 wyłącznie namespace i ścieżkami artefaktów. Brain SHA256 pozostaje `8a38763a0ec5e48f404696dfb0334f678561b244614d2f6cae058ff7562806bc`.

Manifest: `logs/rollout/predator-v11-20260919-candidate-identity-r6/run_config_manifest.json`. Binarki r4/r5 do rollbacku: `/tmp/ghost-candidate-identity-20260919/rollback/`. Kod poprzedniej naprawy świeżości wyjścia nie został zmieniony.

R4 zakończył się z187BUY:104shadow_simulated,60anchor_constraint_error,12slippage_error,1simulation_error oraz10BUY bez wiersza wykonanej symulacji. Zarejestrowano104pozycje,102closed,2unresolved,0active. Suma PnL zamkniętych wynosi -0.096418646SOL. Wynik jest modelem shadow, koszty wykonania pozostają `unmodeled`.

Dwa unresolved r4 z15:29 mają po dwa `rpc_timeout`, a następnie `recovery_deadline_elapsed`. Nie przypisujemy timeoutom szczegółowej przyczyny sieciowej/providera bez dodatkowego dowodu i nie przedstawiamy naprawy aliasów jako naprawy tych timeoutów.

## D7. Przegląd

Przejrzano zmianę względem kopii źródeł z początku zadania, zachowując zastane zmiany w dirty worktree. Kontrola obejmuje aktywne i terminalne indeksy, lifecycle guardów, canonical receipts, normalizację WSOL-base oraz niezmienność raw observation. Poprawka nie dodaje RPC, kolejki, async zadania ani blokującej operacji sieciowej na hot path.

## D8. Wynik operacyjny

Weryfikacja zakończona na r6, snapshot **16:55:52 UTC**, po **6 min 20 s** działania:

| Metryka | Wynik |
|---|---:|
| Wiersze całego logu od startu | 14 114 |
| `candidate identity aliases disagree` | 0 |
| `candidate_integrity_cancelled_before_submit` | 0 |
| Decyzje Gatekeepera | 30 |
| BUY | 3 |
| Udane symulacje / utworzone pozycje | 2 |
| Odrzucona symulacja: slippage | 1 |
| Pozycje zamknięte / aktywne / unresolved | 1 / 1 / 0 |
| PnL zamkniętej pozycji shadow | -0.002156945 SOL |
| Zapisy writera / błędy / dropy | 252 / 0 / 0 |

Pierwsze zamknięcie r6 korzysta z wcześniejszego potwierdzenia RPC: odpowiedź po 94 ms, fill po 500 ms od requestu, wiek canonical sample 30 712 ms. Slot i timestamp canonical pozostały identyczne. Zapisano `exit_filled` oraz `position_closed`, bez `position_unresolved`. Dowód: `/tmp/ghost-candidate-identity-20260919/r6-confirmed-exit.json` oraz lifecycle r6.

Pełny snapshot z offsetami plików: `/tmp/ghost-candidate-identity-20260919/predator-v11-20260919-candidate-identity-r6-status.json`. Zweryfikowano zgodność SHA źródeł i działającej binarki z końcowym buildem. Przegląd diffu zakończony, bez nowych niepowodzeń testów względem baseline. Pomiar dowodzi usunięcia obserwowanego błędu w tym przedziale; nie jest deklaracją braku wszystkich możliwych błędów runtime. Blokady rzeczywistych sprzeczności i niepełnego dowodu pozostają aktywne.

R6 pozostawiony w tmux. Bez commit/push i bez dalszego polling/restartu po przekazaniu wyniku, dopóki użytkownik nie wyda nowej dyspozycji.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE

GO-D nie stanowi wejścia tej naprawy runtime.
