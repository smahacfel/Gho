# ADR-8D: Punktowe potwierdzenie konta odblokowujące wyjście shadow ze StaleSnapshot

## D0. Zakres i zgoda

2026-09-19 użytkownik zatwierdził wdrożenie minimalnej naprawy cichych pozycji shadow. Problem i pomiary providera opisano w ADR_8D_SHADOW_UNRESOLVED_IDLE_ACCOUNT_QUOTE_FRESHNESS_ROOT_CAUSE_20260919.md. Szablon /Gho/docs/ADR/ADR_8D_SZABLON.md i jego odpowiedniki pod /root/Gho oraz /root/Gho_ingest nie istnieją; zachowano lokalny układ D0–D8.

## D1. Problem

Inactivity po 30 s lub absolute max hold po 120 s uruchamiały wyjście, którego canonical sample był starszy niż limit quote 1500 ms. Recovery 5000 ms powtarzało odczyt tego samego snapshotu. Niezmienione konto nie potrzebuje nowej transakcji, aby dało się potwierdzić jego bieżący stan.

## D2. Przyczyna

Brakowało odrębnego dowodu aktualności odczytu niezmienionych rezerw na granicy istniejącego wyjścia. AccountStateReducer poprawnie zachowuje RPC refresh jako observation-only. Nie wolno odświeżać czasu mutacji ani inactivity na podstawie odczytu.

## D3. Rozwiązanie

- Wyłącznie istniejąca propozycja wyjścia shadow z StaleSnapshot może rozpocząć getAccountInfo. RUG i własna ścieżka CrashGuard nie otrzymują tego potwierdzenia.
- Odczyt processed/base64 dotyczy dokładnego PDA krzywej. minContextSlot jest maksimum slotu canonical sample i ostatniego slotu przyjętego przez AccountStateCore ze strumienia.
- Maksymalnie jedno zadanie na monitorowaną pozycję. Pętla nie czeka na sieć; kolejny tick odbiera odpowiedź. Drop pozycji/zadania anuluje odczyt.
- Walidacja obejmuje PDA/base mint, owner Pump, discriminator Anchor, slot kontekstu, długość i BLAKE3 całych bajtów, rezerwy oraz complete=false. Aktualny canonical state musi nadal odpowiadać stanowi, dla którego wysłano zapytanie.
- Odpowiedź jest związana z action_id, position_id, epoch, revision i pozostałą ilością. Guard jest sprawdzany przed lokalnym inkrementem revision kolejnej próby oraz przez istniejący guarded apply wyjścia.
- Dowód obowiązuje tylko w ramach bieżącego recovery i istniejącego TTL 1500 ms liczonego od rozpoczęcia zapytania, również zegarem monotonicznym. Nie jest współdzielony ani przechowywany jako authority kolejnych wycen.
- Zgodny dowód pozwala resolverowi obliczyć wyjście ze zweryfikowanych, niezmienionych rezerw. Niezgodność, wygaśnięcie, brak konta lub błąd RPC nie tworzą fill/PnL. Prawidłowe nowe dane canonical nadal mogą obsłużyć zwykłą ścieżkę wyceny.

## D4. Granice i kontrakty

Bez zmian progów, Gatekeepera, canonical reducer, timestampów konta, inactivity i trajektorii. Warunki TP/SL, inactivity i max hold oraz formuła PnL pozostają dotychczasowe. HET observe-only nadal operuje na własnym zamrożonym zestawie próbek; potwierdzenie dotyczy wykonania istniejącego wyjścia V1.

Lifecycle otrzymuje opcjonalne pole quote_freshness_confirmation. Oryginalne sample slot/timestamp/age zostają w dowodzie; truth_detail identyfikuje rpc_unchanged_account_confirmed. Syntetyczny landed slot dla takiego wyjścia to RPC context slot + 1, z osobnym source label. To nadal symulacja shadow.

Jawne base64 korzysta z już obecnej w workspace biblioteki solana-account-decoder 1.18. Dodano bezpośrednią zależność ghost-brain; bez zmiany wersji w lockfile.

## D5. Weryfikacja

Dowody w /tmp/shadow-quote-confirmation-20260919/. Bazowy zestaw guardian::post_buy:: (przed zmianą engine i launchera): 269 PASS, 2 FAIL. Identyczne błędy dotyczą shadow_v2_exit_fill_preserves_same_slot_ordering_provenance_blocker oraz shadow_v2_exit_fill_uses_lifecycle_pool_state_sell_engine_when_available (klasyfikacja ResearchCandidate/DiagnosticSim); nie są wprowadzone przez tę naprawę.

Nowe testy sprawdzają dodatni/ujemny PnL cichej krzywej, niezmienność canonical/activity, brak potwierdzenia, niezgodne owner/discriminator/hash/slot/PDA, zmienioną pozycję i canonical state, deadline, timeout, brak blokowania ticka, deduplikację i anulowanie, a także rzeczywiste parametry HTTP getAccountInfo. Końcowy zestaw r4: 278 PASS, 2 identyczne wcześniejsze FAIL, 280 testów. Wszystkie 9 nowych testów PASS, w tym test prawdziwego HTTP i ścieżka HET observe-only. Pierwszy release zbudowano i uruchomiono jako r3. Weryfikacja na rzeczywistym ticku 498 ms wykazała utratę gotowej odpowiedzi przed dopuszczoną próbą quote (500 ms). Test 499/999 ms odtworzył błąd (RED), a zachowanie pending read do terminu retry naprawiło go (GREEN). Release r4 zawiera tę poprawkę, bez zmiany TTL ani interwału retry; dowody test-early-tick-red.log, tests-r4-post-buy.log i early-tick-runtime-evidence.json.

## D6. Wdrożenie

Uruchomiony profil /tmp/predator-v11-20260919-quote-confirmation-r4.toml zachowuje wartości r2, zmieniając namespace artefaktów. Końcowy r4 działa od 12:18:50 UTC w tmux ghost-predator-v11-quote-confirmation-20260919-r4, PID522245. Binarka b660093de61b6f145dee01a8707497500ff4fb2feb3c3624e902f4d26db11f9c została zweryfikowana przez /proc; release build i preflight PASS. Plik brain: configs/rollout/ghost_brain_predator_v11_shadow_burnin_20260916.toml, SHA256 8a38763a0ec5e48f404696dfb0334f678561b244614d2f6cae058ff7562806bc. Hash zweryfikowano względem r2 i snapshotu w manifeście logs/rollout/predator-v11-20260919-quote-confirmation-r4/run_config_manifest.json.

## D7. Przegląd i ograniczenia

Przegląd obejmuje guard przed/po async, brak await pod lockiem, pojedyncze zadanie na pozycję, ograniczenie recovery, anulowanie, zgodność bajtów i zachowanie starych pól JSONL. Dowód rzeczywistego mechanizmu w r3: pozycja G8ZPHk8qBkvgW1HpUZgU7KKNB7JhyHnCgakJxL8upump, absolute_max_hold; canonical sample age 28756 ms; request→response 17 ms; request→fill 500 ms; canonical slot 448401012, minContextSlot 448401114, RPC context 448401116. Zapisano exit_filled i position_closed z PnL -0.003105470 SOL oraz terminal_release=released. HET terminal_isolation_violation=false, duplicate_action_observed=false. Finalny r4 dodatkowo usuwa utratę odpowiedzi przy wczesnym ticku; został uruchomiony i jest kontrolowany na aktualnych danych. Zmiana nie przyznaje authority RPC nad canonical state ani nie zamienia shadow PnL w wynik wykonania on-chain.

## D8. Stan końcowy

Wdrożenie i weryfikacja końcowego r4 zakończone. Run pozostawiony w tmux.

Rzeczywisty przypadek r4: mint HdYLfbNvWh1V3w6CBveyrgYRAGmGmVDj5jU9Mop6pump, exit_policy_reason_code=time_stop, powód V1=inactivity. Snapshot miał 31583 ms, inactivity 31500 ms. Po jednej nieudanej próbie RPC ponowienie otrzymało odpowiedź po 84 ms; fill nastąpił po 500 ms od tego requestu i 1499 ms od pierwszego StaleSnapshot, w budżecie 5000 ms. Rzeczywisty wczesny tick 499 ms ma receipt rpc_confirmation_pending, więc odpowiedź nie ginie przed terminem retry.

Canonical slot 448407207 i timestamp 1789820779872 pozostały niezmienione. minContextSlot=448407319, RPC context=448407322. Zapisano exit_filled, position_closed i terminal_release=released. PnL shadow -0.002959612 SOL. HET terminal_isolation_violation=false, duplicate_action_observed=false. Dowód: /tmp/shadow-quote-confirmation-20260919/r4-confirmed-runtime-evidence.json; surowe lifecycle/admission/HET pod logs/shadow_run/predator-v11-20260919-quote-confirmation-r4/.

Końcowy snapshot 12:29:46 UTC: 51 decyzji Gatekeepera, 8 BUY, 7 pozycji monitoring_registered, 6 zamkniętych i zwolnionych, 1 nadal otwarta, 0 position_unresolved. Trzy z sześciu wyjść skorzystały z nowego potwierdzenia RPC. Pozostałe dwa potwierdzone przypadki: BQEmcJsiuBpb8qSJKwhpR2be2Fv2mwfZYP1nwJpypump — sample age31293ms, request→response109ms, request→fill994ms, PnL -0.002371561 SOL; Ht69xQK3Sxgg4seovTUhCm3c61uPGvfpK3FunX6vpump — sample age30633ms, request→response67ms, request→fill500ms, PnL -0.002349043 SOL. Wszystkie trzy mają terminal_release=released i zachowany canonical timestamp. Suma PnL sześciu zamkniętych pozycji: -0.008186354 SOL. Writer HET: writes_failed=0, queue_full_drops=0, queue_closed_drops=0. Snapshoty final-status-r4.json, runtime-verification-r4.json i final-confirmed-positions-r4.json.

Pierwszy błąd odczytu ma zapis rpc_transport_or_response_error; log nie rozróżnia jego przyczyny transportowej od błędu odpowiedzi providera. Nie przypisujemy mu bardziej szczegółowej przyczyny. Odpowiedź błędna nie obsłużyła wyjścia; późniejsza odpowiedź przeszła walidację.

Testy: 9/9 nowych PASS; cały post_buy 278 PASS, 2 FAIL odtworzone na stanie sprzed naprawy. Build release i preflight PASS. Przegląd końcowego diffu oraz guardów czasu/tożsamości, zakresu async i niezmienności canonical zakończony. Bez commit/push. Po zakończeniu obserwacji nie wykonywać dalszego polling/restartu bez nowej dyspozycji użytkownika.

GO_D_SOURCE_AUTHORITY = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE

GO-D nie jest wejściem tej naprawy runtime.
